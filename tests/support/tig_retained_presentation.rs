use super::*;
use reel::tig_retained_presentation::{self as adapter, Request};
use tig_core::tree::{MemoryNodes, TreeLimits};

fn fixture() -> (Fixture, Request) {
    let mut f = Fixture::new();
    let base: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/media.json")).unwrap())
            .unwrap();
    let mut definitions = BTreeMap::new();
    for kind in ["retained", "delivered"] {
        let mut definition = base.clone();
        definition["id"] = json!(format!("types/{kind}"));
        if kind == "retained" {
            definition["authoring"]["properties"] =
                json!(["source", "language", "episode_id", "role", "adoption_slot"]);
        }
        let object = write(f.root(), &format!("definitions/{kind}.json"), &definition);
        f.profile["admitted"].as_array_mut().unwrap().push(
            json!({"id":format!("types/{kind}"),"object":object,"payload":{"kind":"selected"}}),
        );
        definitions.insert(kind, object);
    }
    let delivered = Object::from_bytes(b"synthetic delivered movie; intentionally unavailable");
    let control = serde_json::to_vec(&json!({"synthetic":"metadata"})).unwrap();
    let control_pin = Object::from_bytes(&control);
    fs::write(
        f.root()
            .join("cache/objects")
            .join(control_pin.sha256.as_str()),
        &control,
    )
    .unwrap();
    let file = |name: &str| reference(name, &control_pin);
    let manifest = json!({"schema":"reel.presentation-adopt.v1","role":"film-one","language":"en",
        "season_id":"season","episode_id":"programme","template_id":"film-template",
        "catalog":file("catalog.json"),"template_definition":file("template.json"),
        "season_bindings":file("season.json"),"episode_bindings":file("episode.json"),
        "source_binding":"film","source":reference("old/source.mp4",&delivered),"output_mode":"retain-source"});
    let raw = serde_json::to_vec(&manifest).unwrap();
    let adoption = Object::from_bytes(&raw);
    fs::write(
        f.root()
            .join("cache/objects")
            .join(adoption.sha256.as_str()),
        &raw,
    )
    .unwrap();
    write(
        f.root(),
        "records/delivered.tig",
        &json!({"schema":"tig.instance.design-1","id":"delivered/film",
        "definition":{"id":"types/delivered","sha256":definitions["delivered"].sha256,"bytes":definitions["delivered"].bytes},
        "authored":{},"streams":[],"selected":delivered,"status":{},"metadata":{}}),
    );
    write(
        f.root(),
        "records/reuse.tig",
        &json!({"schema":"tig.instance.design-1","id":"reuse/film/en",
        "definition":{"id":"types/retained","sha256":definitions["retained"].sha256,"bytes":definitions["retained"].bytes},
        "authored":{"source":{"id":"delivered/film"},"language":"en","episode_id":"programme","role":"film-one","adoption_slot":"adoption"},
        "streams":[{"slot":"adoption","role":"control","group":"source","object":adoption}],
        "selected":null,"status":{},"metadata":{}}),
    );
    git(f.root(), &["add", "records", "definitions"]);
    git(f.root(), &["commit", "-m", "synthetic retained output"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    (
        f,
        Request {
            schema: adapter::SCHEMA.into(),
            root_id: "reuse/film/en".into(),
            root_definition: "types/retained".into(),
            source_definitions: ["types/delivered".into()].into_iter().collect(),
        },
    )
}

#[test]
fn retained_output_is_a_composition_subject_without_primary_bytes_or_nodes() {
    let (f, request) = fixture();
    let mut source = f.source();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(prepared.source.id, "delivered/film");
    assert_eq!(prepared.source_file.path, Path::new("delivered/film"));
    assert_eq!(prepared.summary()["primary_reference_verified"], false);
    assert_eq!(prepared.metadata().count(), 5);
    let mut nodes = MemoryNodes::default();
    let context =
        adapter::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default()).unwrap();
    let recipe = adapter::recipe(
        &mut source,
        &request,
        &context,
        &Object::from_bytes(b"explicit producer"),
    )
    .unwrap();
    assert_eq!(recipe.nodes().len(), 1);
    assert!(recipe.nodes().keys().all(|r| r.id == "reuse/film/en"));
    assert!(!f.root().join("exports").exists());
}

#[test]
fn retained_context_replays_without_git_or_source_media() {
    let (f, request) = fixture();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context =
        adapter::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default()).unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let expected = adapter::recipe(
        &mut source,
        &request,
        &context,
        &Object::from_bytes(b"producer"),
    )
    .unwrap();
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut restored, replay) = adapter::restore_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let actual = adapter::recipe(
        &mut restored,
        &request,
        &replay,
        &Object::from_bytes(b"producer"),
    )
    .unwrap();
    assert_eq!(actual.fingerprint(), expected.fingerprint());
}

#[test]
fn changed_delivered_subject_or_scope_refuses_stale_adoption() {
    for (name, pointer, value) in [
        (
            "delivered",
            "/selected",
            serde_json::to_value(Object::from_bytes(b"new movie")).unwrap(),
        ),
        ("reuse", "/authored/role", json!("different-role")),
        ("reuse", "/authored/language", json!("es")),
        ("reuse", "/authored/episode_id", json!("another-programme")),
        ("reuse", "/authored/source/id", json!("cel/a")),
    ] {
        let (mut f, request) = fixture();
        let path = format!("records/{name}.tig");
        let mut record: Value =
            serde_json::from_slice(&fs::read(f.root().join(&path)).unwrap()).unwrap();
        *record.pointer_mut(pointer).unwrap() = value;
        write(f.root(), &path, &record);
        f.commit();
        assert!(adapter::lower(&mut f.source(), &request).is_err());
    }
}

#[cfg(all(windows, feature = "tig-publication"))]
#[test]
fn owned_retained_reference_verifies_media_and_holds_bytes_through_use() {
    use reel::{
        media_runtime,
        tig_presentation::jobs::retained::JobPlan,
        tig_scene_production::jobs::{Coordination, Identity},
    };
    use tig_store::{Key, source_claims::SourceJournal};
    let (mut f, request) = fixture();
    let locate = |name: &str| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(format!("{name}.exe")))
            .find(|p| p.is_file())
            .unwrap()
            .canonicalize()
            .unwrap()
    };
    let movie = f.root().join("movie.mp4");
    assert!(
        Command::new(locate("ffmpeg"))
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=64x64:r=24:d=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-ac",
                "2",
                "-shortest"
            ])
            .arg(&movie)
            .status()
            .unwrap()
            .success()
    );
    let cache = |body: &[u8]| {
        let object = Object::from_bytes(body);
        fs::write(
            f.root().join("cache/objects").join(object.sha256.as_str()),
            body,
        )
        .unwrap();
        object
    };
    let selected = cache(&fs::read(&movie).unwrap());
    let template = cache(
        &serde_json::to_vec(
            &json!({"schema":"reel.selected-presentation-master-template.v1",
        "template_id":"film-template","kind":"series-opening","width":64,"height":64,
        "fps_numerator":24,"fps_denominator":1,"sample_rate":48000,"duration_frames":48}),
        )
        .unwrap(),
    );
    let catalog=cache(&serde_json::to_vec(&json!({"schema":"reel.scene-template-catalog.v1","templates":[{
        "template_id":"film-template","kind":"series-opening","definition_sha256":template.sha256,"required_content_keys":[]}]})).unwrap());
    let season = cache(
        &serde_json::to_vec(
            &json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"season","assets":{}}),
        )
        .unwrap(),
    );
    let episode=cache(&serde_json::to_vec(&json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"programme","assets":{
        "film":{"logical_id":"delivered/film","sha256":selected.sha256,"bytes":selected.bytes,
        "cache_uri":format!("cache://sha256/{}",selected.sha256.as_str()),"selection_state":"selected-private-production"}}})).unwrap());
    let adoption=cache(&serde_json::to_vec(&json!({"schema":"reel.presentation-adopt.v1","role":"film-one","template_kind":"series-opening",
        "language":"en","season_id":"season","episode_id":"programme","template_id":"film-template",
        "catalog":reference("catalog.json",&catalog),"template_definition":reference("template.json",&template),
        "season_bindings":reference("season.json",&season),"episode_bindings":reference("episode.json",&episode),
        "source_binding":"film","source":reference("old/source.mp4",&selected),"output_mode":"retain-source"})).unwrap());
    let load = |name: &str| {
        serde_json::from_slice::<Value>(&fs::read(f.root().join(name)).unwrap()).unwrap()
    };
    let mut delivered = load("records/delivered.tig");
    delivered["selected"] = json!(selected);
    write(f.root(), "records/delivered.tig", &delivered);
    let mut root = load("records/reuse.tig");
    root["streams"][0]["object"] = json!(adoption);
    write(f.root(), "records/reuse.tig", &root);
    let mut definition = load("definitions/delivered.json");
    definition["id"] = json!("types/occurrence");
    definition["authoring"]["properties"] = json!([
        "presentation",
        "episode_id",
        "source_segment_id",
        "language",
        "component_kind"
    ]);
    let definition = write(f.root(), "definitions/occurrence.json", &definition);
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"types/occurrence","object":definition,"payload":{"kind":"selected"}}));
    write(
        f.root(),
        "records/occurrence.tig",
        &json!({"schema":"tig.instance.design-1","id":"occurrence/film/en",
        "definition":{"id":"types/occurrence","sha256":definition.sha256,"bytes":definition.bytes},
        "authored":{"presentation":{"id":request.root_id},"episode_id":"programme","source_segment_id":"film-one",
        "language":"en","component_kind":"episode-presentation"},"selected":selected,"streams":[],"status":{},"metadata":{}}),
    );
    git(f.root(), &["add", "records", "definitions"]);
    git(f.root(), &["commit", "-m", "owned retained fixture"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let tool = |path: PathBuf| {
        let observed = media_runtime::ByteIdentity::observe_file(&path).unwrap();
        reel::scene_delivery::FileRef {
            path,
            sha256: observed.sha256,
            bytes: observed.bytes,
        }
    };
    let programs = media_runtime::BoundPrograms::bind(&media_runtime::Bindings {
        host: media_runtime::ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap(),
        ffmpeg: tool(locate("ffmpeg")),
        ffprobe: tool(locate("ffprobe")),
        dependencies: Default::default(),
    })
    .unwrap();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context =
        adapter::freeze_context(&mut source, &request, &mut nodes, TreeLimits::default()).unwrap();
    let plan = JobPlan::from_context(
        &mut source,
        &request,
        &context,
        &programs,
        "occurrence/film/en",
        "types/occurrence",
    )
    .unwrap();
    let resources = plan.freeze(&mut nodes, TreeLimits::default()).unwrap();
    assert!(resources.objects().contains(&selected));
    let key = |s: &str| Key::parse(s).unwrap();
    let mut journal =
        SourceJournal::create(&f.root().join("journal"), key("fixture"), key("epoch")).unwrap();
    let coordination = Coordination {
        root: f.root().join("coordination"),
    };
    fs::create_dir_all(&coordination.root).unwrap();
    let owner = plan
        .readback(resources.selection(), &mut nodes, TreeLimits::default())
        .unwrap()
        .protect(
            &source,
            &mut journal,
            &coordination,
            Identity {
                source: key("retained-source"),
                owner: key("fixture-owner"),
            },
            0,
            &key("protect"),
            &reel::tig_episode::ProtectionBudget {
                max_objects: 10000,
                max_logical_bytes: 2 * 1024 * 1024 * 1024,
            },
        )
        .unwrap();
    let stage = f.root().join("reference-stage");
    assert!(
        owner
            .verify_selected_master(&mut source, &mut journal, &stage, 0)
            .is_err()
    );
    assert!(!stage.exists());
    let proof = owner
        .verify_selected_master(&mut source, &mut journal, &stage, 1024 * 1024)
        .unwrap();
    assert!(!proof.primary_reference_verified());
    assert_eq!(proof.selected().sha256, selected.sha256.as_str());
    proof.verify_current(&source, &mut journal).unwrap();
    assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
    let held_path = f
        .root()
        .join("cache/objects")
        .join(selected.sha256.as_str());
    assert!(fs::OpenOptions::new().write(true).open(&held_path).is_err());
    drop(proof);
    assert!(fs::OpenOptions::new().write(true).open(&held_path).is_ok());
}
