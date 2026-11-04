#![cfg(all(windows, feature = "tig-publication"))]
//! Actual contained sampler execution over frozen/adopted synthetic Tig inputs.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tig_assets::{
    graph::{self, recipe},
    resolve::AssetResolver,
};
use tig_core::{
    Digest, LogicalPath, Object,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::{GitId, Repository, Scope, native::job};
use tig_store::{
    Key,
    metadata::{Limits, MetadataReader, MetadataStore},
    source_claims::{Command as Claim, SourceJournal},
};

fn git_program() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("git.exe"))
        .find(|p| p.is_file())
        .unwrap()
        .canonicalize()
        .unwrap()
}
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new(git_program())
        .current_dir(root)
        .args([
            "-c",
            "user.name=Synthetic fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=NUL",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn store(cache: &Path, raw: &[u8]) -> Object {
    let object = Object::from_bytes(raw);
    let path = object_path(cache, &object);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, raw).unwrap();
    object
}
fn object_path(cache: &Path, object: &Object) -> PathBuf {
    cache
        .join("objects/sha256")
        .join(&object.sha256.as_str()[..2])
        .join(object.sha256.as_str())
}
fn wave() -> Vec<u8> {
    let mut out = b"RIFF".to_vec();
    out.extend((36 + 3200 * 8u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(3u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(8000u32.to_le_bytes());
    out.extend(64000u32.to_le_bytes());
    out.extend(8u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(b"data");
    out.extend((3200 * 8u32).to_le_bytes());
    for _ in 0..3200 {
        out.extend(0.25f32.to_le_bytes());
        out.extend((-0.125f32).to_le_bytes());
    }
    out
}
fn midi() -> Vec<u8> {
    let conductor = [0, 0xff, 0x51, 3, 0x0f, 0x42, 0x40, 40, 0xff, 0x2f, 0];
    let marker = b"REEL_PART:[\"piano\",\"Piano\"]";
    let mut part = vec![0, 0xff, 3, marker.len() as u8];
    part.extend(marker);
    part.extend([5, 0x90, 60, 64, 20, 0x80, 60, 0, 15, 0xff, 0x2f, 0]);
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend(2u16.to_be_bytes());
    out.extend(100u16.to_be_bytes());
    for track in [&conductor[..], &part] {
        out.extend(b"MTrk");
        out.extend((track.len() as u32).to_be_bytes());
        out.extend(track);
    }
    out
}

#[test]
fn frozen_sampler_job_checks_authority_capacity_and_inputs_then_retains_stems_and_retires_scratch()
{
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repo = root.join("repo");
    let cache = root.join("active");
    let project = root.join("project");
    let coordination = root.join("coordination");
    for path in [
        repo.join("records"),
        repo.join("definitions"),
        cache.join("objects/sha256"),
        project.clone(),
        coordination.clone(),
    ] {
        fs::create_dir_all(path).unwrap();
    }
    let bank = store(&cache, &wave());
    let score = store(&cache, &midi());
    let calibration = store(
        &cache,
        &serde_json::to_vec(&json!({"schema":"reel.music-sample-bank.v1",
        "bank":[{"pitch":60,"audio_attack_frame":0,"held_sample_frames":3000}]}))
        .unwrap(),
    );
    let input = |name: &str, object: &Object| json!({"path":name,"sha256":object.sha256,"bytes":object.bytes});
    let request = json!({"schema":"reel.music-sample-clock-render.v1","bank_audio":input("bank.wav",&bank),
        "calibration":input("calibration.json",&calibration),"native_score_midi":input("score.mid",&score),
        "sample_rate":8000,"channels":2,"frames":3200,"release_frames":400,"gain":1.0,"peak_ceiling":0.89,
        "vocal":{"source":input("vocal.wav",&bank),"gain":0.5}});
    let request_raw = serde_json::to_vec(&request).unwrap();
    let request_pin = store(&cache, &request_raw);
    let bindings = BTreeMap::from([
        ("bank.wav", bank.clone()),
        ("calibration.json", calibration),
        ("score.mid", score),
        ("vocal.wav", bank.clone()),
        ("request.json", request_pin),
    ]);
    let worker = Path::new(env!("CARGO_BIN_EXE_reel"))
        .canonicalize()
        .unwrap();
    let worker_pin = Object::from_bytes(&fs::read(&worker).unwrap());
    let mut absolute = request.clone();
    for (pointer, name) in [
        ("/bank_audio", "bank.wav"),
        ("/calibration", "calibration.json"),
        ("/native_score_midi", "score.mid"),
        ("/vocal/source", "vocal.wav"),
    ] {
        absolute.pointer_mut(pointer).unwrap()["path"] =
            json!(object_path(&cache, &bindings[name]));
    }
    let absolute_request = root.join("absolute.json");
    fs::write(&absolute_request, serde_json::to_vec(&absolute).unwrap()).unwrap();
    let baseline = root.join("baseline");
    let baseline_result = Command::new(&worker)
        .args(["music-sample-clock-render"])
        .arg(&absolute_request)
        .arg("--output-dir")
        .arg(&baseline)
        .output()
        .unwrap();
    assert!(
        baseline_result.status.success(),
        "{}",
        String::from_utf8_lossy(&baseline_result.stderr)
    );
    let outputs:BTreeMap<_,_>=["instrumental.wav","vocal-stem.wav","with-vocals.wav","receipt.json",
        "workspace-request.json","render-request.json","workspace-source.json"].into_iter()
        .map(|name|(name,json!({"path":format!("render/{name}"),"max_bytes":if name=="receipt.json" {3*1024*1024} else {65536}}))).collect();
    let manifest = json!({"budget_bytes":16*1024*1024,"scratch_bytes":1024*1024,"inputs":bindings,"tools":{},
        "installed_tools":{"sampler":{"path":worker,"sha256":worker_pin.sha256,"bytes":worker_pin.bytes}},
        "prepare_output_parents":false,"files":{},
        "steps":[{"tool":"sampler","args":["music-sample-clock-render","request.json","--workspace",".","--output-dir","render","--max-output-bytes","5242880"],"timeout_seconds":30}],
        "outputs":outputs});
    let manifest_raw = serde_json::to_vec(&manifest).unwrap();
    reel_assets::jobs::check_request(&manifest_raw).unwrap();
    let manifest_path = root.join("job.json");
    fs::write(&manifest_path, &manifest_raw).unwrap();
    let schema = br#"{"type":"object"}"#;
    fs::write(repo.join("definitions/authored.json"), schema).unwrap();
    let definition=serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/sample-input",
        "authored_schema":Object::from_bytes(schema),"authoring":{"properties":["bindings"],"required":["bindings"],"defaults":{}},
        "stream_rules":{},"semantic_rules":{}})).unwrap();
    fs::write(repo.join("definitions/input.json"), &definition).unwrap();
    let definition_pin = Object::from_bytes(&definition);
    for (name, object) in &bindings {
        let children = if *name == "request.json" {
            json!({"bank":{"id":"input/bank.wav"},"calibration":{"id":"input/calibration.json"},
            "score":{"id":"input/score.mid"},"vocal":{"id":"input/vocal.wav"}})
        } else {
            json!({})
        };
        let record = json!({"schema":"tig.instance.design-1","id":format!("input/{name}"),
            "definition":{"id":"types/sample-input","sha256":definition_pin.sha256,"bytes":definition_pin.bytes},
            "authored":{"bindings":children},"selected":object,"streams":[],"status":{},"metadata":{}});
        fs::write(
            repo.join("records").join(format!("{name}.tig")),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["add", "records", "definitions"]);
    git(
        &repo,
        &["commit", "--quiet", "-m", "Synthetic sampler inputs"],
    );
    let repository = Repository::new(git_program(), repo.clone(), Default::default()).unwrap();
    let view = repository
        .commit(GitId::parse(&git(&repo, &["rev-parse", "HEAD"])).unwrap())
        .unwrap();
    let scope = |name| Scope::new(vec![LogicalPath::parse(name).unwrap()]).unwrap();
    let mut snapshot = repository
        .native(
            &view,
            scope("records"),
            scope("definitions"),
            Default::default(),
        )
        .unwrap();
    let profiles = [graph::Profile {
        definition_id: "types/sample-input".into(),
        definition: definition_pin.clone(),
        class: graph::NodeKind::Primary,
        covered: BTreeSet::from([graph::EdgeKind::Production]),
        payloads: graph::Payloads::SelectedObject,
        rules: vec![graph::Rule {
            pointer: "/bindings".into(),
            kind: graph::EdgeKind::Production,
            collection: graph::Collection::ObjectValues,
            optional: false,
            binding: graph::Binding::SnapshotSelected,
        }],
    }];
    let query = graph::Query {
        checkpoint: snapshot.checkpoint().clone(),
        roots: BTreeSet::from([snapshot
            .resolve_id("input/request.json")
            .unwrap()
            .reference()
            .clone()]),
        kinds: BTreeSet::from([graph::EdgeKind::Production]),
        limits: graph::Limits {
            nodes: 32,
            edges: 32,
            depth: 8,
            bytes: 1024 * 1024,
        },
    };
    let producer = serde_json::to_vec(
        &json!({"schema":"synthetic.sample-clock-producer.v1","worker":worker_pin}),
    )
    .unwrap();
    let operation = recipe::Operation {
        name: "synthetic.sample-clock-draft".into(),
        semantics: Digest::blob(b"unreviewed workspace sample clock"),
        producer: Object::from_bytes(&producer),
        parameters: Object::from_bytes(&request_raw),
    };
    let graph = graph::resolve(&mut snapshot, &query, &profiles).unwrap();
    let consumption = [recipe::Consumption {
        definition_id: "types/sample-input".into(),
        definition: definition_pin,
        fields: BTreeSet::from(["bindings".into()]),
    }];
    let plan = recipe::plan(
        &graph,
        &mut snapshot,
        &profiles,
        &consumption,
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
    writer.put(&manifest_raw).unwrap();
    let context = snapshot
        .freeze_production_context(
            &query,
            &profiles,
            &request_raw,
            &mut writer,
            TreeLimits::default(),
        )
        .unwrap();
    let runtime = BTreeMap::from([
        (LogicalPath::parse("worker").unwrap(), worker_pin),
        (
            LogicalPath::parse("request").unwrap(),
            Object::from_bytes(&manifest_raw),
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
    let key = |name| Key::parse(name).unwrap();
    let owners = root.join("owners");
    let mut journal = SourceJournal::create(&owners, key("fixture"), key("epoch")).unwrap();
    let prepared = journal
        .apply(
            key("prepare"),
            Claim::Prepare {
                source: key("sampler"),
                owner: key("owner"),
                expected_revision: 0,
                selection: resources.selection().clone(),
            },
        )
        .unwrap();
    journal
        .apply(
            key("adopt"),
            Claim::Adopt {
                source: key("sampler"),
                owner: key("owner"),
                expected_revision: 1,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    let before = journal.checkpoint();
    drop(journal);
    fs::rename(&repo, root.join("repo-offline")).unwrap();
    let profile = json!({"schema":reel::tig_frozen_job_cli::SCHEMA,"project":project,"metadata_root":metadata,"metadata_minimum":minimum,
        "source_context":context.selection(),"source_checkpoint":snapshot.checkpoint(),"profiles":profiles,
        "consumption":consumption,"operation":operation,"runtime":runtime,"job_context":resources.selection(),
        "manifest":{"path":manifest_path,"sha256":Object::from_bytes(&manifest_raw).sha256,"bytes":manifest_raw.len()},
        "cache":{"path_policy":"configured-local-roots","cache_roots":[cache],"coordination_root":coordination,
            "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}},
        "journal_root":owners,"journal_minimum":before,"source_id":"sampler","owner":"owner","expected_revision":2,"retain_operation":"retain"});
    let profile_path = root.join("run.json");
    let invoke = |value: &Value| {
        fs::write(&profile_path, serde_json::to_vec(value).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("run-frozen")
            .arg(&profile_path)
            .output()
            .unwrap()
    };
    for changed in ["owner", "revision", "capacity"] {
        let mut bad = profile.clone();
        match changed {
            "owner" => bad["owner"] = json!("foreign"),
            "revision" => bad["expected_revision"] = json!(1),
            "capacity" => bad["cache"]["capacity"]["minimum_free_bytes"] = json!(1_u64 << 52),
            _ => unreachable!(),
        }
        // Every negative profile must pass canonical intake so capacity tests
        // reach admission rather than merely failing an unsafe JSON integer.
        tig_core::identity::parse_json(&serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(!invoke(&bad).status.success());
        assert_eq!(
            SourceJournal::open(&owners, &before).unwrap().checkpoint(),
            before
        );
    }
    fs::write(object_path(&cache, &bank), b"corrupt synthetic source").unwrap();
    assert!(!invoke(&profile).status.success());
    assert_eq!(
        SourceJournal::open(&owners, &before).unwrap().checkpoint(),
        before
    );
    fs::write(object_path(&cache, &bank), wave()).unwrap();
    let out = invoke(&profile);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["production_claim"], Value::Null);
    assert_eq!(result["creative_approval"], false);
    assert_eq!(result["workspace_retained"], false);
    assert_eq!(result["outputs"].as_object().unwrap().len(), 7);
    for name in ["instrumental.wav", "vocal-stem.wav", "with-vocals.wav"] {
        let retained = fs::read(result["outputs"][name]["path"].as_str().unwrap()).unwrap();
        assert_eq!(retained, fs::read(baseline.join(name)).unwrap());
        assert_eq!(
            Object::from_bytes(&retained).sha256.as_str(),
            result["outputs"][name]["sha256"].as_str().unwrap()
        );
    }
    let source: Value = serde_json::from_slice(
        &fs::read(
            result["outputs"]["workspace-source.json"]["path"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(source["check"]["exact_stem_sum_verified"], true);
    assert_eq!(source["check"]["vocal_pcm_verified"], true);
    assert!(source["job_admission"].is_null());
    let state = SourceJournal::open(&owners, &before)
        .unwrap()
        .source(&key("sampler"))
        .unwrap()
        .unwrap();
    assert_eq!(state.revision, 4);
}
