use reel::{media_runtime::ByteIdentity, scene_delivery::Receipt, tig_scene::Request};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};
use tig_core::{
    LogicalPath, Object,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::{
    Key,
    metadata::{Checkpoint, MetadataReader},
    source_claims::{Selection, SourceJournal},
};

fn check_candidates(root: &Path, completed: &Value, original_record: &[u8], template: &Value) {
    use tig_assets::{
        graph::Reference,
        graph::recipe::produced::{BoundSubject, Claim, Selector},
        native::Instance,
    };
    let rendered = completed["result"]["outputs"].as_object().unwrap();
    let exported = &completed["record_candidates"];
    assert_eq!(exported["current_selection"], false);
    let record = &exported["records"][0];
    assert_eq!(&record["template"], template);
    let bytes = fs::read(record["retained_path"].as_str().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&bytes)).unwrap(),
        record["record"]
    );
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["status"].as_object().unwrap().is_empty());
    assert_eq!(value["authored"], json!({"label":"Native scene delivery"}));
    assert!(value["metadata"].get("prior_review").is_none());
    assert_eq!(&value["metadata"]["production"]["template"], template);
    assert_eq!(value["metadata"]["production"]["claim"], exported["claim"]);
    assert_eq!(
        value["selected"],
        json!({"sha256":rendered["native/master.mkv"]["sha256"],"bytes":rendered["native/master.mkv"]["bytes"]})
    );
    let claim_bytes =
        fs::read(rendered["production-claim.json"]["path"].as_str().unwrap()).unwrap();
    let claim = Claim::parse(
        &serde_json::from_value(exported["claim"].clone()).unwrap(),
        &claim_bytes,
    )
    .unwrap();
    assert_eq!(claim.outputs().len(), 2);
    let schema = fs::read(root.join("cli-offline-definitions/authored.json")).unwrap();
    let definition = fs::read(root.join("cli-offline-definitions/native-output.json")).unwrap();
    let bound = Instance::parse(&bytes)
        .unwrap()
        .bind(&definition, &schema)
        .unwrap();
    let admission = Reference {
        id: "types/native-output".into(),
        object: Object::from_bytes(&definition),
    };
    for selector in [
        Selector::Selected {},
        Selector::Stream { slot: "es".into() },
    ] {
        let subject = BoundSubject::bind(&bound, &admission, selector)
            .unwrap()
            .unwrap();
        assert!(claim.outputs().contains(subject.subject()));
    }
    let original: Value = serde_json::from_slice(original_record).unwrap();
    assert_eq!(value["streams"][0], original["streams"][0]);
    assert_eq!(
        fs::read(root.join("cli-offline-records/native-output.tig")).unwrap(),
        original_record
    );
    let mut annotated = value;
    annotated["metadata"]["review_note"] = json!("candidate reviewed later");
    let annotated = Instance::parse(&serde_json::to_vec(&annotated).unwrap())
        .unwrap()
        .bind(&definition, &schema)
        .unwrap();
    let subject = BoundSubject::bind(&annotated, &admission, Selector::Selected {})
        .unwrap()
        .unwrap();
    assert!(claim.outputs().contains(subject.subject()));
}

pub fn seed(f: &mut super::Fixture) {
    let root = f.root().to_path_buf();
    let schema = Object::from_bytes(&fs::read(root.join("definitions/authored.json")).unwrap());
    let definition = super::write(
        &root,
        "definitions/native-output.json",
        &json!({"schema":"tig.definition.design-1","id":"types/native-output","authored_schema":schema,"authoring":{"properties":["label"],"required":["label"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
    );
    let history = b"previous synthetic native output";
    let old = Object::from_bytes(history);
    fs::write(
        root.join("cache/objects").join(old.sha256.as_str()),
        history,
    )
    .unwrap();
    super::write(
        &root,
        "records/native-output.tig",
        &json!({"schema":"tig.instance.design-1","id":"delivery/native-scene","definition":{"id":"types/native-output","sha256":definition.sha256,"bytes":definition.bytes},"authored":{"label":"Native scene delivery"},"selected":null,"streams":[{"slot":"history-v1","role":"provenance","group":"history","object":old}],"status":{"selection_state":"principal-approved"},"metadata":{"prior_review":"not applicable to replacement bytes"}}),
    );
    super::git(&root, &["add", "definitions", "records"]);
    super::git(&root, &["commit", "-m", "plan native output record"]);
    f.profile["commit"] = json!(super::git(&root, &["rev-parse", "HEAD"]));
    f.profile["admitted"].as_array_mut().unwrap().push(
        json!({"id":"types/native-output","object":definition,"payload":{"kind":"selected"}}),
    );
}

pub fn exercise(
    root: &Path,
    request: &Request,
    context: &Selection,
    metadata: (&Path, &Checkpoint),
    source: Value,
    reference: &Receipt,
) {
    let locate = |name: &str| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join(format!("{name}.exe")))
            .find(|p| p.is_file())
            .unwrap()
            .canonicalize()
            .unwrap()
    };
    let file = |path: std::path::PathBuf| {
        let observed = ByteIdentity::observe_file(&path).unwrap();
        json!({"path":path,"sha256":observed.sha256,"bytes":observed.bytes})
    };
    let binary = Path::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .canonicalize()
        .unwrap();
    let host = ByteIdentity::observe_file(&binary).unwrap();
    let cache = root.join("cli-active");
    fs::create_dir_all(cache.join("objects/sha256")).unwrap();
    let control = root.join("cli-control");
    fs::create_dir(&control).unwrap();
    let journal_root = root.join("cli-owner");
    let journal = SourceJournal::create(
        &journal_root,
        Key::parse("fixture").unwrap(),
        Key::parse("epoch").unwrap(),
    )
    .unwrap();
    let witness = journal.checkpoint();
    drop(journal);
    let mut outputs: BTreeMap<_, _> = reference
        .outputs
        .keys()
        .map(|n| (n.clone(), 4 * 1024 * 1024_u64))
        .collect();
    outputs.insert("receipt.json".into(), 4 * 1024 * 1024);
    let profile = json!({
        "schema":"reel.tig-native-job-profile.experimental-1",
        "worker":{"metadata_root":metadata.0,"metadata_minimum":metadata.1,"source":source,"context":context,"request":request,
            "runtime":{"host":host,"ffmpeg":file(locate("ffmpeg")),"ffprobe":file(locate("ffprobe")),"dependencies":{}},"outputs":outputs},
        "cache":{"path_policy":"configured-local-roots","cache_roots":[cache],"coordination_root":control,
            "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}},
        "journal_root":journal_root,"journal_minimum":witness,"source_id":"native-cli","owner":"worker-owner","expected_revision":0,
        "start_operation":"start","retain_operation":"retain","budget_bytes":256*1024*1024_u64,"scratch_bytes":64*1024*1024_u64,"timeout_seconds":120,
    });
    let original_record = fs::read(root.join("records/native-output.tig")).unwrap();
    let template =
        json!({"id":"delivery/native-scene","object":Object::from_bytes(&original_record)});
    let path = root.join("native-cli-profile.json");
    let run = || {
        Command::new(&binary)
            .arg("run")
            .arg(&path)
            .output()
            .unwrap()
    };
    let before = MetadataReader::open(metadata.0, metadata.1)
        .unwrap()
        .checkpoint()
        .clone();
    let mut invalid = profile.clone();
    invalid["budget_bytes"] = json!(1);
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let rejected = run();
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("request refused"),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(
        SourceJournal::open(&journal_root, &witness)
            .unwrap()
            .checkpoint(),
        witness
    );
    assert_eq!(
        MetadataReader::open(metadata.0, metadata.1)
            .unwrap()
            .checkpoint(),
        &before
    );
    assert!(!cache.join("staging/caminos").exists());
    for refusal in ["stale-template", "production-input"] {
        let mut rejected = profile.clone();
        let selected = if refusal == "stale-template" {
            json!({"id":"delivery/native-scene","object":Object::from_bytes(b"wrong template bytes")})
        } else {
            json!({"id":"cel/a","object":Object::from_bytes(&fs::read(root.join("records/cel-a.tig")).unwrap())})
        };
        rejected["worker"]["record_outputs"] =
            json!([{"output":"master.mkv","template":selected,"selector":{"kind":"selected"}}]);
        fs::write(&path, serde_json::to_vec(&rejected).unwrap()).unwrap();
        let result = run();
        assert!(!result.status.success());
        let message = String::from_utf8_lossy(&result.stderr);
        assert!(
            message.contains(if refusal == "stale-template" {
                "not admitted"
            } else {
                "production input record"
            }),
            "{message}"
        );
        assert_eq!(
            SourceJournal::open(&journal_root, &witness)
                .unwrap()
                .checkpoint(),
            witness
        );
        assert_eq!(
            MetadataReader::open(metadata.0, metadata.1)
                .unwrap()
                .checkpoint(),
            &before
        );
        assert!(!cache.join("staging/caminos").exists());
    }
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    for name in [".git", "records", "definitions"] {
        fs::rename(root.join(name), root.join(format!("cli-offline-{name}"))).unwrap();
    }
    let result = run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        result["schema"],
        "reel.tig-native-job-result.experimental-1"
    );
    assert!(result["production_claim"].is_null());
    assert_eq!(result["complete_claim_coverage"], false);
    assert_eq!(result["result"]["workspace_retained"], false);
    let retained = &result["result"]["outputs"];
    for name in outputs
        .keys()
        .map(|n| format!("native/{n}"))
        .chain(std::iter::once("worker.json".into()))
    {
        let item = &retained[&name];
        let observed =
            ByteIdentity::observe_file(Path::new(item["path"].as_str().unwrap())).unwrap();
        assert_eq!(observed.sha256, item["sha256"]);
        assert_eq!(observed.bytes, item["bytes"]);
    }
    let receipt: Receipt = serde_json::from_slice(
        &fs::read(retained["native/receipt.json"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt.content_samples, reference.content_samples);
    assert_eq!(receipt.delivery_frames, reference.delivery_frames);
    assert_eq!(
        receipt.outputs.keys().collect::<Vec<_>>(),
        reference.outputs.keys().collect::<Vec<_>>()
    );
    let completion: Value = serde_json::from_slice(
        &fs::read(retained["worker.json"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        completion["receipt"],
        json!({"sha256":retained["native/receipt.json"]["sha256"],"bytes":retained["native/receipt.json"]["bytes"]})
    );
    assert!(
        fs::read_dir(cache.join("staging/caminos"))
            .unwrap()
            .next()
            .is_none()
    );
    let mut journal = SourceJournal::open(
        &journal_root,
        &serde_json::from_value(result["journal_checkpoint"].clone()).unwrap(),
    )
    .unwrap();
    let state = journal
        .source(&Key::parse("native-cli").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(state.revision, 4);
    assert_eq!(state.predecessors.len(), 1);
    assert!(!journal.obligations(100).unwrap().complete_claim_coverage());
    drop(journal);
    // Refuse a mismatched declared set before rendering any media. Retain
    // failure evidence, with no generated candidates, before owned cleanup.
    let mut incomplete = profile.clone();
    incomplete["source_id"] = json!("native-cli-output-mismatch");
    incomplete["worker"]["outputs"]["not-produced.bin"] = json!(1024);
    fs::write(&path, serde_json::to_vec(&incomplete).unwrap()).unwrap();
    let failed = run();
    assert!(!failed.status.success());
    let error = String::from_utf8_lossy(&failed.stderr);
    assert!(
        error.contains("staging retired; diagnostic") && error.contains("cache://sha256/"),
        "{error}"
    );
    let mut journal = SourceJournal::open(&journal_root, &witness).unwrap();
    let state = journal
        .source(&Key::parse("native-cli-output-mismatch").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(state.revision, 4);
    assert_eq!(state.predecessors.len(), 1);
    let mut latest = MetadataReader::open(metadata.0, metadata.1).unwrap();
    let header = Tree::from_root(state.current.unwrap().selection.root)
        .get(
            &LogicalPath::parse("retention/identity").unwrap(),
            &mut latest,
            TreeLimits::default(),
        )
        .unwrap()
        .0
        .unwrap();
    let header: Value = serde_json::from_slice(&latest.get(&header).unwrap().unwrap()).unwrap();
    assert_eq!(header["succeeded"], false);
    assert_eq!(header["outputs"], 0);
    assert!(header["production_claim"].is_null());
    assert_eq!(
        journal
            .source(&Key::parse("native-cli").unwrap())
            .unwrap()
            .unwrap()
            .revision,
        4
    );
    assert!(
        fs::read_dir(cache.join("staging/caminos"))
            .unwrap()
            .next()
            .is_none()
    );
    drop(journal);
    for mode in ["plan-only", "automatic"] {
        let mut automatic = profile.clone();
        automatic["source_id"] = json!(format!("native-cli-{mode}"));
        automatic["scratch_bytes"] = json!(128 * 1024 * 1024_u64);
        automatic["worker"]
            .as_object_mut()
            .unwrap()
            .remove("outputs");
        automatic["worker"]["output_budget"] =
            json!({"default_max_bytes":4*1024*1024_u64,"overrides":{"receipt.json":65_536}});
        if mode == "automatic" {
            automatic["worker"]["record_outputs"] = json!([
                {"output":"master.mkv","template":template,"selector":{"kind":"selected"}},
                {"output":"master.mkv","template":template,"selector":{"kind":"stream","slot":"es"},"stream_class":{"role":"compiled-output","group":"language"}}
            ]);
        }
        fs::write(&path, serde_json::to_vec(&automatic).unwrap()).unwrap();
        let completed = Command::new(&binary)
            .arg(if mode == "plan-only" {
                "plan-outputs"
            } else {
                "run"
            })
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            completed.status.success(),
            "{}",
            String::from_utf8_lossy(&completed.stderr)
        );
        let completed: Value = serde_json::from_slice(&completed.stdout).unwrap();
        let report = &completed["planning"];
        assert_eq!(
            report["contract"]["names"],
            json!(reference.outputs.keys().collect::<Vec<_>>())
        );
        assert_eq!(
            report["contract"]["raw_pcm24_stereo_bytes"],
            reference.content_samples * 6
        );
        let maxima = report["outputs"].as_object().unwrap();
        assert_eq!(maxima.len(), reference.outputs.len() + 1);
        assert_eq!(maxima["receipt.json"], 65_536);
        assert_eq!(
            report["atomic_native_output_bytes"],
            maxima.values().map(|n| n.as_u64().unwrap()).sum::<u64>() * 2
        );
        let planner = if mode == "plan-only" {
            &completed["result"]
        } else {
            &completed["planning_result"]
        };
        let planned = planner["outputs"].as_object().unwrap();
        assert!(
            planner["budget_bytes"].as_u64().unwrap()
                < automatic["scratch_bytes"].as_u64().unwrap()
        );
        assert!(
            planner["budget_bytes"].as_u64().unwrap() < profile["budget_bytes"].as_u64().unwrap()
        );
        assert_eq!(planned.len(), 1);
        let item = &planned["planning.json"];
        let retained: Value =
            serde_json::from_slice(&fs::read(item["path"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(&retained, report);
        assert_eq!(
            ByteIdentity::observe_file(Path::new(item["path"].as_str().unwrap()))
                .unwrap()
                .sha256,
            item["sha256"]
        );
        let mut journal = SourceJournal::open(
            &journal_root,
            &serde_json::from_value(completed["journal_checkpoint"].clone()).unwrap(),
        )
        .unwrap();
        let state = journal
            .source(&Key::parse(&format!("native-cli-{mode}")).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(state.revision, if mode == "plan-only" { 4 } else { 8 });
        assert_eq!(
            state.predecessors.len(),
            if mode == "plan-only" { 1 } else { 3 }
        );
        assert_eq!(completed["result"]["workspace_retained"], false);
        if mode == "automatic" {
            assert_eq!(completed["result"]["budget_bytes"], profile["budget_bytes"]);
            let rendered = completed["result"]["outputs"].as_object().unwrap();
            assert_eq!(rendered.len(), reference.outputs.len() + 5);
            for item in rendered.values() {
                let bytes =
                    ByteIdentity::observe_file(Path::new(item["path"].as_str().unwrap())).unwrap();
                assert_eq!(bytes.sha256, item["sha256"]);
                assert_eq!(bytes.bytes, item["bytes"]);
            }
        }
        if mode == "automatic" {
            check_candidates(root, &completed, &original_record, &template);
        }
        assert!(
            fs::read_dir(cache.join("staging/caminos"))
                .unwrap()
                .next()
                .is_none()
        );
        assert!(!journal.obligations(100).unwrap().complete_claim_coverage());
    }
    assert!(!control.join("catalog.json").exists());
    assert!(!control.join("registry").exists());
    for name in [".git", "records", "definitions"] {
        fs::rename(root.join(format!("cli-offline-{name}")), root.join(name)).unwrap();
    }
}
