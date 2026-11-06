use super::*;

pub(in crate::song_edition) struct Fixture {
    pub(in crate::song_edition) dir: tempfile::TempDir,
    pub(in crate::song_edition) admission: Value,
}
impl Fixture {
    fn put(&self, name: &str, bytes: &[u8]) -> Value {
        let path = self.dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        json!({"path":path,"object":{"sha256":sha256_bytes(bytes),"bytes":bytes.len()}})
    }
    pub(in crate::song_edition) fn file(&self, key: &str) -> PathBuf {
        PathBuf::from(self.admission[key]["path"].as_str().unwrap())
    }
    pub(in crate::song_edition) fn change(&mut self, key: &str, edit: impl FnOnce(&mut Value)) {
        let mut value: Value = packet::parse(&fs::read(self.file(key)).unwrap()).unwrap();
        edit(&mut value);
        let raw = serde_json::to_vec(&value).unwrap();
        self.admission[key] = self.put(&format!("changed-{key}.json"), &raw);
        if key != "original_package" && self.admission["original_package"].is_object() {
            self.relink_package();
        }
    }
    fn inspect(&self) -> Result<VerifiedOriginal> {
        let path = self.dir.path().join("admission.json");
        fs::write(&path, serde_json::to_vec(&self.admission).unwrap()).unwrap();
        super::inspect(&path)
    }
    fn relink_request(&mut self) {
        let hash = self.admission["original_request"]["object"]["sha256"].clone();
        self.change("original_generation", |g| g["request_hash"] = hash);
    }
    fn relink_package(&mut self) {
        let request = self.admission["original_request"]["object"].clone();
        let generation = self.admission["original_generation"]["object"].clone();
        let recording = self.admission["recording"]["object"].clone();
        self.change("original_package", |p| {
            p["assets"]["request"] = request;
            p["assets"]["generation"] = generation;
            p["assets"]["recording"] = recording;
        });
    }
}
pub(in crate::song_edition) fn fixture() -> Fixture {
    let mut f = Fixture {
        dir: tempfile::tempdir().unwrap(),
        admission: Value::Null,
    };
    let text = "Álpha beta\nDelta echo";
    let canonical = f.put("source.txt", text.as_bytes());
    let request = f.put("request.json", &serde_json::to_vec(&json!({
        "schema":"example.older-request.v1","words":text,"sung":"[Verse]\nÁlpha beta\nDelta echo",
        "private":true,"upload":false,"imitation":false,"release":false
    })).unwrap());
    let audio = wave::encode(&[0.25, -0.125].repeat(8000), 8000, 2).unwrap();
    let recording = f.put("original.wav", &audio);
    let generation = f.put("generation.json", &serde_json::to_vec(&json!({
        "schema":"example.older-result.v2","request_hash":request["object"]["sha256"],
        "text_hash":sha256_bytes(text.as_bytes()),
        "render_hash":sha256_bytes("[Verse]\nÁlpha beta\nDelta echo".as_bytes()),
        "private_candidate":true,"upload":false,
        "outputs":[{"path":"original-name.wav","sha256":recording["object"]["sha256"],"bytes":audio.len()}]
    })).unwrap());
    let package = f.put("package.json", &serde_json::to_vec(&json!({
        "schema":"example.song-package.v1","version":"v3","assets":{
            "request":request["object"],"generation":generation["object"],"recording":recording["object"]}
    })).unwrap());
    f.admission = json!({"schema":"reel.song-source-interpretation.v1",
        "source_version":"v3","original_package":package,
        "original_request":request,"original_generation":generation,"canonical_text":canonical,"recording":recording,
        "interpretation":{"package_schema":"example.song-package.v1","package_version_pointer":"/version",
        "package_request_pointer":"/assets/request","package_generation_pointer":"/assets/generation",
        "package_recording_pointer":"/assets/recording",
        "request_schema":"example.older-request.v1","generation_schema":"example.older-result.v2",
        "canonical_lyrics_pointer":"/words","render_lyrics_pointer":"/sung",
        "private_only_pointer":"/private","external_upload_pointer":"/upload",
        "identifiable_imitation_pointer":"/imitation","public_release_pointer":"/release",
        "generation_request_sha256_pointer":"/request_hash","generation_canonical_sha256_pointer":"/text_hash",
        "generation_render_sha256_pointer":"/render_hash",
        "generation_outputs_pointer":"/outputs","generation_private_flag_pointer":"/private_candidate",
        "generation_external_upload_pointer":"/upload"}});
    f
}
#[test]
fn older_original_stays_original_and_new_context_cannot_claim_approval() {
    let f = fixture();
    let original = fs::read(f.file("original_request")).unwrap();
    let generation = fs::read(f.file("original_generation")).unwrap();
    let verified = f.inspect().unwrap();
    assert_eq!(
        verified.context["schema"],
        "reel.song-repair-source-context.v1"
    );
    assert_eq!(
        verified.context["canonical_lyrics"],
        "Álpha beta\nDelta echo"
    );
    assert_eq!(
        verified.context["original_render_lyrics"],
        "[Verse]\nÁlpha beta\nDelta echo"
    );
    assert_eq!(verified.context["source_frames"], 8000);
    assert_eq!(verified.context["source_version"], "v3");
    for key in [
        "mapping_authority_verified",
        "original_request_rewritten",
        "performed_words_verified",
        "voice_consent_granted",
        "provider_executed",
        "human_approval",
        "shareable",
    ] {
        assert_eq!(verified.context[key], false, "{key}");
    }
    let out = f.dir.path().join("context.json");
    verified.write(&out).unwrap();
    assert!(verified.write(&out).is_err());
    assert_eq!(fs::read(f.file("original_request")).unwrap(), original);
    assert_eq!(fs::read(f.file("original_generation")).unwrap(), generation);
}
#[test]
fn changed_raw_bytes_and_broken_historical_links_refuse() {
    let f = fixture();
    fs::write(f.file("recording"), b"different").unwrap();
    assert!(f.inspect().is_err());
    for (key, bad) in [
        ("request_hash", json!("0".repeat(64))),
        ("text_hash", json!("0".repeat(64))),
        ("render_hash", json!("0".repeat(64))),
        ("outputs", json!([])),
        ("outputs", json!([{"sha256":"0".repeat(64),"bytes":4}])),
    ] {
        let mut f = fixture();
        f.change("original_generation", |g| g[key] = bad);
        assert!(f.inspect().is_err(), "accepted {key}");
    }
    let mut f = fixture();
    f.change("original_generation", |g| {
        let output = g["outputs"][0].clone();
        g["outputs"].as_array_mut().unwrap().push(output);
    });
    assert!(f.inspect().is_err());
}
#[test]
fn forbidden_or_missing_permissions_and_unknown_schemas_refuse() {
    for key in ["upload", "imitation", "release"] {
        let mut f = fixture();
        f.change("original_request", |r| r[key] = json!(true));
        f.relink_request();
        assert!(
            f.inspect()
                .err()
                .unwrap()
                .to_string()
                .contains("private restriction")
        );
    }
    for value in [Value::Null, json!(false), json!("true")] {
        let mut f = fixture();
        f.change("original_request", |r| r["private"] = value);
        f.relink_request();
        assert!(
            f.inspect()
                .err()
                .unwrap()
                .to_string()
                .contains("private restriction")
        );
    }
    for (key, value) in [("private_candidate", json!(false)), ("upload", json!(true))] {
        let mut f = fixture();
        f.change("original_generation", |g| g[key] = value);
        assert!(
            f.inspect()
                .err()
                .unwrap()
                .to_string()
                .contains("private restriction")
        );
    }
    let mut f = fixture();
    f.admission["interpretation"]["request_schema"] = json!("other.schema");
    assert!(f.inspect().is_err());
    let mut f = fixture();
    f.admission["interpretation"]["public_release_pointer"] = json!("/upload");
    assert!(f.inspect().is_err());
}
#[test]
fn source_normalization_is_explicit_and_only_one_terminal_lf() {
    let mut f = fixture();
    let raw = fs::read(f.file("canonical_text")).unwrap();
    f.admission["canonical_text"] = f.put("source-lf.txt", &[raw.as_slice(), b"\n"].concat());
    assert!(f.inspect().is_err());
    f.admission["interpretation"]["allow_single_terminal_lf_difference"] = json!(true);
    assert!(f.inspect().is_ok());
    for suffix in [b"\n\n".as_slice(), b"\r\n", b" ", b" changed"] {
        f.admission["canonical_text"] =
            f.put("source-other.txt", &[raw.as_slice(), suffix].concat());
        assert!(f.inspect().is_err(), "accepted {suffix:?}");
    }
}
#[test]
fn duplicate_keys_invalid_pointers_and_relative_inputs_refuse() {
    let mut f = fixture();
    let raw = String::from_utf8(fs::read(f.file("original_request")).unwrap()).unwrap();
    let duplicate = raw.replacen('{', "{\"private\":true,", 1);
    f.admission["original_request"] = f.put("duplicate.json", duplicate.as_bytes());
    f.relink_request();
    assert!(f.inspect().is_err());
    let mut f = fixture();
    f.admission["interpretation"]["canonical_lyrics_pointer"] = json!("");
    assert!(f.inspect().is_err());
    let mut f = fixture();
    f.admission["recording"]["path"] = json!("relative.wav");
    assert!(f.inspect().is_err());
}

#[test]
fn original_package_version_and_exact_members_are_required() {
    let mut f = fixture();
    f.admission["source_version"] = json!("v4");
    assert!(
        f.inspect()
            .err()
            .unwrap()
            .to_string()
            .contains("package version")
    );
    for key in ["request", "generation", "recording"] {
        let mut f = fixture();
        f.change("original_package", |p| {
            p["assets"][key]["sha256"] = json!("0".repeat(64))
        });
        assert!(
            f.inspect()
                .err()
                .unwrap()
                .to_string()
                .contains("package version")
        );
    }
}
