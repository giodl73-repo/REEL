use super::*;
use serde_json::{Value, json};

fn pin(name: &str, bytes: &[u8]) -> Value {
    json!({"path":name,"sha256":sha(bytes),"bytes":bytes.len()})
}

/// Rehashing modified evidence also updates its invocation pin. Negative cases
/// therefore exercise semantic checks, rather than merely a stale JSON hash.
fn fixture(
    language: &str,
    alter_evidence: impl FnOnce(&mut Value),
    alter_episode: impl FnOnce(&mut Value),
) -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let text = if language == "es" {
        "Texto original: canción."
    } else {
        "Selected original text."
    };
    let master_pin = pin("master.mkv", b"selected rendered source");
    let picture_hash = sha(b"selected picture");
    let mut evidence = json!({
        "schema":"reel.presentation-source-text.v1","episode_id":"episode",
        "role":"photo","language":language,"picture_binding":"photo.picture",
        "picture_sha256":picture_hash,"caption_template_id":"caption",
        "rendered_source_master_sha256":master_pin["sha256"],
        "units":[{"source_id":"book-block-1","text":text,"sha256":sha(text.as_bytes())}]
    });
    alter_evidence(&mut evidence);
    let evidence_raw = serde_json::to_vec(&evidence).unwrap();
    let evidence_pin = pin("text.json", &evidence_raw);
    let mut episode = json!({
        "schema":"reel.episode-authoring.v1","episode_id":"episode","season_id":"season",
        "authoring_state":"ready-for-private-build","master_template_id":"master",
        "scene_policy_id":"narrative","score_palette":[],"scene_ids":[],
        "presentation":[{"role":"photo","template_id":"caption","content":{
            "picture_binding":"photo.picture","source_text_evidence_sha256_by_language":{
                language:evidence_pin["sha256"]
            }
        }}]
    });
    alter_episode(&mut episode);
    let definition = json!({"schema":"reel.episode-master-template.v2","template_id":"master",
        "ordered_units":[{"kind":"episode-presentation","id":"photo","source_ids":["book-block-1"],
            "source_text_sha256_by_language":{
                "es":[sha("Texto original: canción.".as_bytes())],
                "en":[sha(b"Selected original text.")]
            }}]
    });
    let bindings = json!({"schema":"reel.scoped-bindings.v1","scope_id":"episode","assets":{
        "photo.picture":{"logical_id":"photo","sha256":picture_hash,"bytes":16,
            "cache_uri":format!("cache://sha256/{picture_hash}"),"selection_state":"selected-private-production"}
    }});
    let mut files = BTreeMap::from([("text.json".into(), evidence_raw)]);
    for (name, value) in [
        ("episode.json", episode),
        ("master.json", definition),
        ("bindings.json", bindings),
        (
            "season.json",
            json!({"schema":"reel.scoped-bindings.v1","scope_id":"season","assets":{}}),
        ),
    ] {
        files.insert(name.into(), serde_json::to_vec(&value).unwrap());
    }
    let manifest = serde_json::from_value(json!({
        "schema":SCHEMA,"episode_id":"episode","language":language,"output_sample_rate":48000,
        "catalog":pin("catalog.json",b"not consumed here"),
        "master_template_definition":pin("master.json",&files["master.json"]),
        "episode":pin("episode.json",&files["episode.json"]),
        "season_bindings":pin("season.json",&files["season.json"]),
        "episode_bindings":pin("bindings.json",&files["bindings.json"]),
        "segments":[{"kind":"episode-presentation","id":"photo","master":master_pin,
            "source_receipt":pin("reference.json",b"not authority in preflight"),"source_text_evidence":evidence_pin}]
    })).unwrap();
    (manifest, files)
}

fn check(manifest: &Manifest, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    preflight_native_invocations(manifest, |reference| {
        let name = reference.path.to_str().context("non-text metadata name")?;
        let raw = files.get(name).context("missing admitted evidence")?;
        if pin(name, raw)["sha256"] != reference.sha256 || raw.len() as u64 != reference.bytes {
            bail!("metadata pin differs");
        }
        Ok(raw.clone())
    })
}

#[test]
fn accepts_exact_bilingual_contract_without_a_legacy_receipt() {
    for language in ["es", "en"] {
        let (manifest, files) = fixture(language, |_| {}, |_| {});
        check(&manifest, &files).unwrap();
        assert!(!files.contains_key("reference.json"));
    }
}

#[test]
fn rejects_rehashed_but_semantically_changed_evidence() {
    for (pointer, replacement) in [
        ("/episode_id", json!("other")),
        ("/role", json!("closing")),
        ("/language", json!("en")),
        ("/caption_template_id", json!("other")),
        ("/picture_binding", json!("other")),
        ("/picture_sha256", json!("a".repeat(64))),
        ("/rendered_source_master_sha256", json!("b".repeat(64))),
        ("/rendered_source_master_sha256", Value::Null),
        ("/units/0/source_id", json!("book-block-2")),
        ("/units/0/text", json!("Changed original text")),
        ("/units/0/sha256", json!("c".repeat(64))),
    ] {
        let (manifest, files) = fixture(
            "es",
            |e| *e.pointer_mut(pointer).unwrap() = replacement,
            |_| {},
        );
        assert!(check(&manifest, &files).is_err(), "accepted {pointer}");
    }
}

#[test]
fn rejects_ignored_content_missing_evidence_and_changed_current_master() {
    for content in [json!({}), Value::Null, json!({"unexpected":"ignored"})] {
        let (manifest, files) =
            fixture("es", |_| {}, |e| e["presentation"][0]["content"] = content);
        assert!(check(&manifest, &files).is_err());
    }
    let (manifest, files) = fixture(
        "es",
        |_| {},
        |e| e["presentation"][0]["content"]["extra"] = true.into(),
    );
    assert!(check(&manifest, &files).is_err());
    let (mut manifest, mut files) = fixture("es", |_| {}, |_| {});
    manifest.segments[0].master.sha256 = "d".repeat(64);
    assert!(check(&manifest, &files).is_err());
    let (manifest, _) = fixture("es", |_| {}, |_| {});
    files.remove("text.json");
    assert!(check(&manifest, &files).is_err());
}

#[test]
fn rejects_duplicate_picture_resolution_and_missing_selected_language_pin() {
    let (mut manifest, mut files) = fixture("es", |_| {}, |_| {});
    let mut season: Value = serde_json::from_slice(&files["season.json"]).unwrap();
    let episode: Value = serde_json::from_slice(&files["bindings.json"]).unwrap();
    season["assets"] = episode["assets"].clone();
    let raw = serde_json::to_vec(&season).unwrap();
    manifest.season_bindings = serde_json::from_value(pin("season.json", &raw)).unwrap();
    files.insert("season.json".into(), raw);
    assert!(check(&manifest, &files).is_err());

    let (manifest, files) = fixture(
        "es",
        |_| {},
        |e| {
            e["presentation"][0]["content"]["source_text_evidence_sha256_by_language"] =
                json!({"en":"a".repeat(64)});
        },
    );
    assert!(check(&manifest, &files).is_err());
}

#[test]
fn non_text_invocations_keep_empty_content_and_reject_unconsumed_fields() {
    let (mut manifest, files) = fixture("es", |_| {}, |_| {});
    let episode: Episode = serde_json::from_slice(&files["episode.json"]).unwrap();
    let bindings: ScopedBindings = serde_json::from_slice(&files["bindings.json"]).unwrap();
    let season: ScopedBindings = serde_json::from_slice(&files["season.json"]).unwrap();
    let mut invocation = episode.presentation[0].clone();
    invocation.content = json!({});
    manifest.segments[0].source_text_evidence = None;
    verify_native_display(
        None,
        &manifest.segments[0],
        &manifest,
        &invocation,
        [&bindings, &season],
        None,
    )
    .unwrap();
    invocation.content = json!({"unconsumed":"new display instructions"});
    assert!(
        verify_native_display(
            None,
            &manifest.segments[0],
            &manifest,
            &invocation,
            [&bindings, &season],
            None
        )
        .is_err()
    );
}
