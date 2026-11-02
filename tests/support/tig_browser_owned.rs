use super::*;
use reel::{
    media_runtime::{BoundPrograms, BoundTool},
    tig_scene_production::jobs::{Coordination, Identity},
};
use tig_core::tree::MemoryNodes;
use tig_store::{Key, source_claims::SourceJournal};

pub fn exercise(
    f: &Fixture,
    profile: &Value,
    master: &Object,
    programs: &BoundPrograms,
    browser: &BoundTool,
    limits: &reel::tig_browser::render::Limits,
) {
    let root = f.root().join("owned-browser-project");
    fs::create_dir(&root).unwrap();
    for (original, current) in [
        ("offline-records", "records"),
        ("offline-definitions", "definitions"),
    ] {
        fs::create_dir(root.join(current)).unwrap();
        for entry in fs::read_dir(f.root().join(original)).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            fs::copy(entry.path(), root.join(current).join(entry.file_name())).unwrap();
        }
    }
    fs::write(root.join(".gitattributes"), "* text eol=lf\n").unwrap();
    let mut definition: Value =
        serde_json::from_slice(&fs::read(root.join("definitions/native-output.json")).unwrap())
            .unwrap();
    definition["authoring"]["properties"]
        .as_array_mut()
        .unwrap()
        .extend([json!("presentation"), json!("episode_id")]);
    let definition = write(&root, "definitions/native-output.json", &definition);
    let mut record: Value =
        serde_json::from_slice(&fs::read(root.join("records/native-output.tig")).unwrap()).unwrap();
    record["definition"] =
        json!({"id":"types/native-output","sha256":definition.sha256,"bytes":definition.bytes});
    record["authored"]["presentation"] = json!({"id":"browser/source/es"});
    record["authored"]["episode_id"] = json!("fixture-episode");
    record["authored"]["source_segment_id"] = json!("opening");
    record["authored"]["language"] = json!("es");
    record["authored"]["component_kind"] = json!("episode-presentation");
    record["selected"] = json!(master);
    record["status"] = json!({});
    write(&root, "records/native-output.tig", &record);
    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Synthetic owned browser"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    git(&root, &["add", "."]);
    git(
        &root,
        &["commit", "-m", "bind synthetic browser occurrence"],
    );
    let mut enrolled = f.profile.clone();
    enrolled["repository"] = json!(root);
    enrolled["commit"] = json!(git(&root, &["rev-parse", "HEAD"]));
    for admission in enrolled["admitted"].as_array_mut().unwrap() {
        if admission["id"] == "types/native-output" {
            admission["object"] = json!(definition);
        }
    }
    enrolled["byte_source"]["places"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "root":f.root().join("capture-capacity"),"layout":"reel-cas"
        }));
    let mut source = Source::open(serde_json::from_value(enrolled.clone()).unwrap()).unwrap();
    let request = serde_json::from_value(profile["worker"]["request"].clone()).unwrap();
    let mut metadata = MemoryNodes::default();
    let (_, context) =
        reel::tig_browser::freeze_context(&mut source, &request, &mut metadata, Default::default())
            .unwrap();
    let plan = reel::tig_presentation::jobs::browser::JobPlan::from_context(
        &mut source,
        &request,
        &context,
        programs,
        browser,
        limits,
        false,
        "delivery/native-scene",
        "types/native-output",
    )
    .unwrap();
    let resources = plan.freeze(&mut metadata, Default::default()).unwrap();
    assert!(resources.objects().contains(master));
    let mut journal = SourceJournal::open(
        Path::new(profile["journal_root"].as_str().unwrap()),
        &serde_json::from_value(profile["journal_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let coordination = Coordination {
        root: PathBuf::from(profile["cache"]["coordination_root"].as_str().unwrap()),
    };
    let identity = Identity {
        source: Key::parse("owned-browser-reference").unwrap(),
        owner: Key::parse("fixture-owner").unwrap(),
    };
    let budget = reel::tig_episode::ProtectionBudget {
        max_objects: 10000,
        max_logical_bytes: 2 * 1024 * 1024 * 1024,
    };
    let owner = plan
        .readback(resources.selection(), &mut metadata, Default::default())
        .unwrap()
        .protect(
            &source,
            &mut journal,
            &coordination,
            identity.clone(),
            0,
            &Key::parse("protect-owned-browser").unwrap(),
            &budget,
        )
        .unwrap();
    let competing = plan
        .readback(resources.selection(), &mut metadata, Default::default())
        .unwrap()
        .protect(
            &source,
            &mut journal,
            &coordination,
            identity,
            owner.revision(),
            &Key::parse("competing-owner").unwrap(),
            &budget,
        );
    assert!(competing.is_err());
    drop(competing);
    let stage = f.root().join("owned-browser-reference-stage");
    fs::create_dir(&stage).unwrap();
    let proof = owner
        .verify_selected_master(
            &mut source,
            &mut journal,
            &stage,
            limits.max_workspace_bytes,
        )
        .unwrap();
    assert_eq!(proof.selected().sha256, master.sha256.as_str());
    assert_eq!(proof.source().id, "browser/source/es");
    assert_eq!(proof.output().id, "delivery/native-scene");
    proof.verify_current(&source, &mut journal).unwrap();
    assert_eq!(stage.read_dir().unwrap().count(), 0);
    record["metadata"]["revision_note"] = json!("another source checkpoint");
    write(&root, "records/native-output.tig", &record);
    git(&root, &["add", "records"]);
    git(&root, &["commit", "-m", "change source view after proof"]);
    enrolled["commit"] = json!(git(&root, &["rev-parse", "HEAD"]));
    let changed = Source::open(serde_json::from_value(enrolled).unwrap()).unwrap();
    assert!(proof.verify_current(&changed, &mut journal).is_err());
}
