#![cfg(feature = "tig")]
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_core::Object;
fn file(root: &Path, name: &str, bytes: &[u8]) -> Value {
    fs::write(root.join(name), bytes).unwrap();
    let object = Object::from_bytes(bytes);
    json!({"path":name,"sha256":object.sha256,"bytes":object.bytes})
}
struct Fixture {
    root: tempfile::TempDir,
    profile: Value,
    original: Value,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let schema = file(root.path(), "schema.json", br#"{"type":"object"}"#);
        let schema_object = json!({"sha256":schema["sha256"],"bytes":schema["bytes"]});
        let def = serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/control",
            "authored_schema":schema_object,"authoring":{"properties":["label"],"required":["label"],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
        let definition = file(root.path(), "definition.json", &def);
        let old = Object::from_bytes(b"old private control unavailable");
        let original = json!({"schema":"tig.instance.design-1","id":"control/one",
            "definition":{"id":"types/control","sha256":definition["sha256"],"bytes":definition["bytes"]},
            "authored":{"label":"keep exact original authoring"},"selected":{"sha256":old.sha256,"bytes":old.bytes,"media_type":"application/json"},
            "streams":[{"slot":"previous","role":"source/history","group":"history","object":old,"provenance":{"private":"preserved"}}],
            "status":{"selection_state":"selected-private-production","human_approval":true,"subject":old,"restriction":"private-only"},
            "metadata":{"rights":{"public_release":false},"authority":{"subject":old}}});
        let record = file(
            root.path(),
            "original.tig",
            &serde_json::to_vec(&original).unwrap(),
        );
        let replacement = file(
            root.path(),
            "replacement.json",
            br#"{"holds":[1,2],"native_decimal":0.25}"#,
        );
        let edit = file(
            root.path(),
            "edit.json",
            br#"{"schema":"synthetic.edit.v1","human_approval":false}"#,
        );
        let profile = json!({"schema":reel::tig_control_edit::SCHEMA,"asset_id":"control/one","original_record":record,
            "definition":{"definition":definition,"authored_schema":schema},"expected_selected":old,
            "replacement":replacement,"edit_request":edit,"source_checkpoint":Object::from_bytes(b"declared checkpoint").sha256,
            "max_control_bytes":4096});
        Self {
            root,
            profile,
            original,
        }
    }
    fn invoke(&self) -> std::process::Output {
        fs::write(
            self.root.path().join("profile.json"),
            serde_json::to_vec(&self.profile).unwrap(),
        )
        .unwrap();
        Command::new(env!("CARGO_BIN_EXE_reel-tig-register"))
            .args(["--control-edit", "profile.json", "candidate.tig"])
            .current_dir(self.root.path())
            .env("PATH", "")
            .output()
            .unwrap()
    }
}
#[test]
fn candidate_preserves_identity_authoring_rights_history_and_resets_approval() {
    let f = Fixture::new();
    let original = fs::read(f.root.path().join("original.tig")).unwrap();
    let out = f.invoke();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    for private in [
        "control/one",
        "sha256",
        "private-only",
        f.root.path().to_str().unwrap(),
    ] {
        assert!(!text.contains(private));
    }
    let summary: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(summary["outputs_retained"], false);
    assert_eq!(summary["source_ownership_verified"], false);
    let candidate = fs::read(f.root.path().join("candidate.tig")).unwrap();
    let c: Value = serde_json::from_slice(&candidate).unwrap();
    for field in ["id", "definition", "authored", "streams"] {
        assert_eq!(c[field], f.original[field]);
    }
    assert_eq!(c["selected"]["sha256"], f.profile["replacement"]["sha256"]);
    assert_eq!(c["selected"]["media_type"], "application/json");
    assert_eq!(c["status"]["selection_state"], "draft-unreviewed");
    assert_eq!(c["status"]["human_approval"], false);
    assert_eq!(c["metadata"]["rights"], f.original["metadata"]["rights"]);
    assert_eq!(
        c["metadata"]["authority"],
        f.original["metadata"]["authority"]
    );
    assert_eq!(
        c["metadata"]["reel_control_edit"]["predecessor_status"],
        f.original["status"]
    );
    assert_eq!(
        c["metadata"]["reel_control_edit"]["production_claim"],
        Value::Null
    );
    assert_eq!(
        fs::read(f.root.path().join("original.tig")).unwrap(),
        original
    );
    assert!(!f.invoke().status.success());
    assert_eq!(
        fs::read(f.root.path().join("candidate.tig")).unwrap(),
        candidate
    );
}
#[test]
fn malformed_or_unpinned_control_edits_refuse_without_candidate_or_private_errors() {
    for case in [
        "unknown",
        "original",
        "payload",
        "selection",
        "definition",
        "budget",
        "not-json",
        "request",
        "traversal",
        "planned",
        "unchanged",
        "target",
        "payload-name",
        "reserved-name",
        "existing-payload",
        "too-large-limit",
        "conflicting-lineage",
    ] {
        let mut f = Fixture::new();
        match case {
            "unknown" => f.profile["future"] = json!(true),
            "target" => f.profile["asset_id"] = json!("other/control"),
            "payload-name" => f.profile["payload_filename"] = json!("../escape.json"),
            "reserved-name" => f.profile["payload_filename"] = json!("NUL.json"),
            "existing-payload" => f.profile["payload_filename"] = json!("replacement.json"),
            "too-large-limit" => f.profile["max_control_bytes"] = json!(4 * 1024 * 1024 + 1),
            "conflicting-lineage" => {
                f.original["metadata"]["reel_control_edit"] = json!({"schema":"caller-owned"});
                f.profile["original_record"] = file(
                    f.root.path(),
                    "original.tig",
                    &serde_json::to_vec(&f.original).unwrap(),
                );
            }
            "original" => {
                f.profile["original_record"]["sha256"] =
                    json!(Object::from_bytes(b"foreign").sha256)
            }
            "payload" => f.profile["replacement"]["bytes"] = json!(999),
            "selection" => {
                f.profile["expected_selected"] = json!(Object::from_bytes(b"foreign selection"))
            }
            "definition" => {
                f.profile["definition"]["definition"]["sha256"] =
                    json!(Object::from_bytes(b"foreign def").sha256)
            }
            "budget" => f.profile["max_control_bytes"] = json!(1),
            "not-json" => {
                f.profile["replacement"] =
                    file(f.root.path(), "replacement.json", b"not a JSON control")
            }
            "request" => f.profile["edit_request"] = file(f.root.path(), "edit.json", b"[]"),
            "traversal" => f.profile["replacement"]["path"] = json!("../escape.json"),
            "planned" => {
                f.original["selected"] = Value::Null;
                f.profile["original_record"] = file(
                    f.root.path(),
                    "original.tig",
                    &serde_json::to_vec(&f.original).unwrap(),
                );
            }
            _ => {
                let bytes = br#"{"unchanged":true}"#;
                let pin = Object::from_bytes(bytes);
                f.original["selected"] = json!(pin);
                f.profile["expected_selected"] = json!(pin);
                f.profile["original_record"] = file(
                    f.root.path(),
                    "original.tig",
                    &serde_json::to_vec(&f.original).unwrap(),
                );
                f.profile["replacement"] = file(f.root.path(), "replacement.json", bytes);
            }
        }
        let out = f.invoke();
        assert!(!out.status.success(), "{case}");
        assert!(!f.root.path().join("candidate.tig").exists(), "{case}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim(),
            "control edit candidate refused; verify exact inputs, definition, draft and new output",
            "{case}"
        );
    }
}

#[test]
fn explicit_control_copy_is_exact_and_never_replaces_an_existing_payload() {
    let mut f = Fixture::new();
    f.profile["payload_filename"] = json!("retained-control.json");
    let out = f.invoke();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["payload_copied"], true);
    assert_eq!(
        fs::read(f.root.path().join("retained-control.json")).unwrap(),
        fs::read(f.root.path().join("replacement.json")).unwrap()
    );
    fs::remove_file(f.root.path().join("candidate.tig")).unwrap();
    let retained = fs::read(f.root.path().join("retained-control.json")).unwrap();
    assert!(!f.invoke().status.success());
    assert!(!f.root.path().join("candidate.tig").exists());
    assert_eq!(
        fs::read(f.root.path().join("retained-control.json")).unwrap(),
        retained
    );
}

#[test]
#[cfg(all(windows, feature = "tig-publication"))]
fn managed_control_edit_retains_both_objects_before_an_isolated_git_proposal() {
    use reel::tig_job_runner::{self, Profile as Runner};
    use reel::tig_scene_production::jobs::{Coordination, Identity};
    use std::collections::{BTreeMap, BTreeSet};
    use tig_assets::{
        graph::{self, recipe},
        resolve::AssetResolver,
    };
    use tig_core::{
        Digest, LogicalPath,
        tree::{NodeStore, Tree, TreeLimits},
    };
    use tig_git::source::{GitId, Repository, Scope, native::job, proposal};
    use tig_store::{
        Key,
        metadata::{Limits, MetadataReader, MetadataStore},
        source_claims::{Command as Claim, SourceJournal},
    };
    let mut f = Fixture::new();
    f.profile["payload_filename"] = json!("retained-control.json");
    let root = f.root.path();
    let repo_root = root.join("repo");
    fs::create_dir_all(repo_root.join("records")).unwrap();
    fs::create_dir(repo_root.join("definitions")).unwrap();
    for (from, to) in [
        ("original.tig", "records/input.tig"),
        ("definition.json", "definitions/control.json"),
        ("schema.json", "definitions/authored.json"),
    ] {
        fs::copy(root.join(from), repo_root.join(to)).unwrap();
    }
    let git_exe = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("git.exe"))
        .find(|p| p.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let git = |args: &[&str]| {
        let out = Command::new(&git_exe)
            .args(args)
            .current_dir(&repo_root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.name", "Synthetic fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    git(&["add", "records", "definitions"]);
    git(&["commit", "-m", "original control"]);
    let base = GitId::parse(&git(&["rev-parse", "HEAD"])).unwrap();
    let repository =
        Repository::new(git_exe.clone(), repo_root.clone(), Default::default()).unwrap();
    let view = repository.commit(base.clone()).unwrap();
    let scope = |name| Scope::new(vec![LogicalPath::parse(name).unwrap()]).unwrap();
    let mut snapshot = repository
        .native(
            &view,
            scope("records"),
            scope("definitions"),
            Default::default(),
        )
        .unwrap();
    f.profile["source_checkpoint"] = json!(snapshot.checkpoint());
    let worker_profile = serde_json::to_vec(&f.profile).unwrap();
    let worker = Path::new(env!("CARGO_BIN_EXE_reel-tig-register"))
        .canonicalize()
        .unwrap();
    let worker_object = Object::from_bytes(&fs::read(&worker).unwrap());
    let mut files = serde_json::Map::new();
    for name in [
        "original.tig",
        "definition.json",
        "schema.json",
        "replacement.json",
        "edit.json",
    ] {
        files.insert(
            name.into(),
            json!(fs::read_to_string(root.join(name)).unwrap()),
        );
    }
    files.insert(
        "worker-profile.json".into(),
        json!(String::from_utf8(worker_profile.clone()).unwrap()),
    );
    let manifest = json!({"budget_bytes":worker_object.bytes+4*1024*1024,"scratch_bytes":65536,
        "inputs":{},"tools":{},"installed_tools":{"worker":{"path":worker,"sha256":worker_object.sha256,"bytes":worker_object.bytes}},
        "files":files,"steps":[{"tool":"worker","args":["--control-edit","worker-profile.json","candidate.tig"],"timeout_seconds":30}],
        "outputs":{"record":{"path":"candidate.tig","max_bytes":65536},"control":{"path":"retained-control.json","max_bytes":4096}}});
    let request = serde_json::to_vec(&manifest).unwrap();
    let producer = serde_json::to_vec(&json!({"schema":"synthetic.control-ingress.v1","worker":worker_object,"request":Object::from_bytes(&request)})).unwrap();
    let record = snapshot.resolve_id("control/one").unwrap();
    let definition = record.definition().object.clone();
    let profiles = [graph::Profile {
        definition_id: "types/control".into(),
        definition: definition.clone(),
        class: graph::NodeKind::Primary,
        covered: BTreeSet::from([graph::EdgeKind::Production]),
        rules: vec![],
        payloads: graph::Payloads::SelectedObject,
    }];
    let query = graph::Query {
        checkpoint: snapshot.checkpoint().clone(),
        roots: BTreeSet::from([record.reference().clone()]),
        kinds: BTreeSet::from([graph::EdgeKind::Production]),
        limits: graph::Limits {
            nodes: 32,
            edges: 32,
            depth: 8,
            bytes: 1024 * 1024,
        },
    };
    let graph = graph::resolve(&mut snapshot, &query, &profiles).unwrap();
    let operation = recipe::Operation {
        name: "synthetic.control-ingress".into(),
        semantics: Digest::blob(b"unproven authored-control ingress"),
        producer: Object::from_bytes(&producer),
        parameters: Object::from_bytes(&worker_profile),
    };
    let plan = recipe::plan(
        &graph,
        &mut snapshot,
        &profiles,
        &[recipe::Consumption {
            definition_id: "types/control".into(),
            definition,
            fields: BTreeSet::from(["label".into()]),
        }],
        &operation,
        recipe::Limits {
            nodes: 32,
            edges: 32,
            bytes: 1024 * 1024,
        },
    )
    .unwrap();
    let metadata = root.join("metadata");
    let mut writer = MetadataStore::create(
        &metadata,
        Limits {
            nodes: 8192,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    writer.put(&producer).unwrap();
    writer.put(&request).unwrap();
    let context = snapshot
        .freeze_production_context(
            &query,
            &profiles,
            &worker_profile,
            &mut writer,
            TreeLimits::default(),
        )
        .unwrap();
    let runtime = BTreeMap::from([
        (LogicalPath::parse("worker").unwrap(), worker_object),
        (
            LogicalPath::parse("request").unwrap(),
            Object::from_bytes(&request),
        ),
    ]);
    let admission = job::Admission {
        context: &context,
        plan: &plan,
        runtime: &runtime,
        output: None,
    };
    let candidate = job::freeze(&admission, &mut writer, TreeLimits::default()).unwrap();
    let minimum = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(&metadata, &minimum).unwrap();
    let resources = job::restore(
        candidate.selection(),
        &admission,
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    let key = |s| Key::parse(s).unwrap();
    let owners = root.join("owners");
    let mut journal = SourceJournal::create(&owners, key("fixture"), key("epoch")).unwrap();
    let identity = Identity {
        source: key("edit"),
        owner: key("owner"),
    };
    let prepared = journal
        .apply(
            key("prepare"),
            Claim::Prepare {
                source: identity.source.clone(),
                owner: identity.owner.clone(),
                expected_revision: 0,
                selection: resources.selection().clone(),
            },
        )
        .unwrap();
    journal
        .apply(
            key("adopt"),
            Claim::Adopt {
                source: identity.source.clone(),
                owner: identity.owner.clone(),
                expected_revision: 1,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    let cache = root.join("active");
    fs::create_dir_all(cache.join("objects/sha256")).unwrap();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    let coordination = Coordination {
        root: root.join("coordination"),
    };
    fs::create_dir(&coordination.root).unwrap();
    let mut config = reel_assets::Config {
        path_policy: reel_assets::PathPolicy::ConfiguredLocalRoots,
        cache_roots: vec![cache.clone()],
        coordination_root: Some(coordination.root.clone()),
        ..Default::default()
    };
    config.capacity.minimum_free_bytes = 0;
    config.capacity.minimum_free_basis_points = 0;
    let mut runner = Runner {
        config: &config,
        coordination: &coordination,
        identity: identity.clone(),
        expected_revision: 2,
        operation: key("retain"),
        metadata_root: &metadata,
        metadata_minimum: &minimum,
        tree_limits: TreeLimits::default(),
    };
    let manifest_path = root.join("job.json");
    fs::write(&manifest_path, &request).unwrap();
    let before = journal.checkpoint();
    runner.identity.owner = key("foreign");
    assert!(
        tig_job_runner::run(&project, &manifest_path, &resources, &mut journal, &runner).is_err()
    );
    assert_eq!(journal.checkpoint(), before);
    runner.identity.owner = identity.owner.clone();
    let frozen = json!({"schema":reel::tig_frozen_job_cli::SCHEMA,"project":project,
        "metadata_root":metadata,"metadata_minimum":minimum,"source_context":context.selection(),
        "source_checkpoint":snapshot.checkpoint(),"profiles":profiles,
        "consumption":[{"definition_id":"types/control","definition":profiles[0].definition,"fields":["label"]}],
        "operation":operation,"runtime":runtime,"job_context":resources.selection(),
        "manifest":{"path":manifest_path,"sha256":Object::from_bytes(&request).sha256,"bytes":request.len()},
        "cache":{"path_policy":"configured-local-roots","cache_roots":[cache],"coordination_root":coordination.root,
            "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}},"journal_root":owners,"journal_minimum":before,
        "source_id":"edit","owner":"owner","expected_revision":2,"retain_operation":"retain"});
    drop(journal);
    let frozen_path = root.join("frozen.json");
    let invoke = |profile: &Value| {
        fs::write(&frozen_path, serde_json::to_vec(profile).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("run-frozen")
            .arg(&frozen_path)
            .output()
            .unwrap()
    };
    for changed in [
        "owner",
        "revision",
        "checkpoint",
        "request",
        "runtime",
        "selection",
        "consumption",
        "schema",
    ] {
        let mut bad = frozen.clone();
        match changed {
            "owner" => bad["owner"] = json!("foreign"),
            "revision" => bad["expected_revision"] = json!(1),
            "checkpoint" => bad["source_checkpoint"] = json!(Digest::blob(b"wrong")),
            "request" => bad["manifest"]["sha256"] = json!(Digest::blob(b"wrong")),
            "runtime" => bad["runtime"]["request"] = json!(Object::from_bytes(b"wrong")),
            "selection" => bad["job_context"]["context"] = json!(Digest::blob(b"wrong")),
            "consumption" => bad["consumption"][0]["fields"] = json!([]),
            "schema" => bad["schema"] = json!("unknown"),
            _ => unreachable!(),
        }
        let rejected = invoke(&bad);
        assert!(!rejected.status.success(), "{changed}");
        assert!(rejected.stdout.is_empty());
        assert!(
            !String::from_utf8_lossy(&rejected.stderr)
                .contains(&root.to_string_lossy().to_string())
        );
        let reopened = SourceJournal::open(&owners, &before).unwrap();
        assert_eq!(reopened.checkpoint(), before);
    }
    let executed = invoke(&frozen);
    assert!(
        executed.status.success(),
        "{}",
        String::from_utf8_lossy(&executed.stderr)
    );
    let result: Value = serde_json::from_slice(&executed.stdout).unwrap();
    assert_eq!(result["production_claim"], Value::Null);
    assert_eq!(result["creative_approval"], false);
    assert_eq!(result["workspace_retained"], false);
    let record_bytes = fs::read(result["outputs"]["record"]["path"].as_str().unwrap()).unwrap();
    let control_bytes = fs::read(result["outputs"]["control"]["path"].as_str().unwrap()).unwrap();
    let new_record: Value = serde_json::from_slice(&record_bytes).unwrap();
    let control = Object::from_bytes(&control_bytes);
    assert_eq!(new_record["selected"]["sha256"], json!(control.sha256));
    assert_eq!(new_record["status"]["selection_state"], "draft-unreviewed");
    let journal = SourceJournal::open(&owners, &before).unwrap();
    let checkpoint = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&owners, &checkpoint).unwrap();
    let state = journal.source(&identity.source).unwrap().unwrap();
    assert_eq!(state.revision, 4);
    let mut latest = MetadataReader::open(&metadata, &minimum).unwrap();
    let entries = Tree::from_root(state.current.unwrap().selection.root)
        .entries(&mut latest, TreeLimits::default())
        .unwrap()
        .0;
    for (name, pin) in [
        ("record", Object::from_bytes(&record_bytes)),
        ("control", control),
    ] {
        assert_eq!(
            entries[&LogicalPath::parse(&format!(
                "outputs/{}",
                Digest::blob(name.as_bytes()).as_str()
            ))
            .unwrap()],
            pin
        );
    }
    let header: Value = serde_json::from_slice(
        &latest
            .get(&entries[&LogicalPath::parse("retention/identity").unwrap()])
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(header["production_claim"], Value::Null);
    assert_eq!(header["succeeded"], true);
    let replacements = [proposal::Replacement {
        path: LogicalPath::parse("records/input.tig").unwrap(),
        expected: Object::from_bytes(&fs::read(root.join("original.tig")).unwrap()),
        bytes: record_bytes,
    }];
    let proposal = repository
        .propose(
            base.clone(),
            &replacements,
            &proposal::Identity {
                name: "Synthetic fixture".into(),
                email: "fixture@example.invalid".into(),
                unix_seconds: 1_700_000_000,
            },
            "unreviewed control edit",
        )
        .unwrap();
    assert_eq!(git(&["rev-parse", "HEAD"]), base.as_str());
    assert_eq!(
        git(&[
            "diff",
            "--name-only",
            base.as_str(),
            proposal.commit().as_str()
        ]),
        "records/input.tig"
    );
    assert_eq!(
        git(&["rev-parse", proposal.reference()]),
        proposal.commit().as_str()
    );
    assert!(
        fs::read_dir(cache.join("staging/caminos"))
            .unwrap()
            .next()
            .is_none()
    );
}
