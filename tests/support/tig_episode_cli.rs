//! Real CLI -> admitted process tree -> native reference proofs -> retained movie.
use super::*;
use reel::media_runtime::ByteIdentity;
use std::io::Read;
use tig_core::tree::NodeStore;
use tig_store::source_claims::Selection as SourceSelection;

#[test]
#[ignore = "requires native FFmpeg; bounded episode CLI with captured source and old jobs offline"]
fn bounded_episode_cli_reopens_source_verifies_components_and_retains_checked_movie() {
    let mut p = Project::new();
    let scenes = (0..4)
        .map(|i| p.render_delivery(i, "cli-v1"))
        .collect::<Vec<_>>();
    p.assemble(&scenes, "cli-v1");
    let request = add_episode_assembly(&mut p, "episode-cli-v1.json");
    let _parent_controls = parent_controls::exercise(&p, &request);
    let mut source = p.source();
    let metadata = p.data.path().join("episode-cli-metadata");
    let mut writer = MetadataStore::create(
        &metadata,
        tig_store::metadata::Limits {
            nodes: 60000,
            bytes: 32 * 1024 * 1024,
            database_bytes: 64 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let context = reel::tig_episode::freeze_composition_context(
        &mut source,
        &request,
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let minimum = writer.finish().unwrap();
    drop(writer);
    let archive = json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),"checkpoint":source.checkpoint(),"admitted":p.profile["admitted"],"byte_source":p.profile["byte_source"]});
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_reel-tig-job"))
        .canonicalize()
        .unwrap();
    let mut runtime = serde_json::to_value(&p.runtime).unwrap();
    runtime["host"] = serde_json::to_value(ByteIdentity::observe_file(&binary).unwrap()).unwrap();
    let config = json!({"path_policy":"configured-local-roots","cache_roots":[p.hot()],"coordination_root":p.data.path().join("coordination"),"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}});
    let inputs = json!({"source":archive,"context":context.selection(),"request":request,"runtime":runtime,
        "scenes":p.requests.iter().zip(UNITS).map(|(request,id)|json!({"id":id,"request":request,"output_id":format!("output/{id}"),"output_definition":"types/output","compiled":id=="scene-b"})).collect::<Vec<_>>(),"presentations":[]});
    let profile = json!({"schema":"reel.tig-episode-job.experimental-1","inputs":inputs,"cache":config,
        "preparation_cache":p.hot(),"metadata_root":metadata,"metadata_minimum":minimum,
        "journal_root":p.data.path().join("owners"),"journal_minimum":p.owners.checkpoint(),
        "source_id":"bounded-episode-cli","owner":"fixture","expected_revision":0,"operation":"run-v1",
        "budget_bytes":256*1024*1024,"scratch_bytes":96*1024*1024,"timeout_seconds":240,
        "reference_staging_bytes":16*1024*1024,"reference_picture_processes":16,
        "max_objects":10000,"max_logical_bytes":1024*1024*1024,
        "outputs":{"master.mkv":16*1024*1024,"receipt.json":1024*1024}});
    let profile_path = p.data.path().join("bounded-episode-profile.json");
    write(p.data.path(), "bounded-episode-profile.json", &profile);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            p.repo.path().join(name),
            p.repo.path().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    fs::rename(
        p.data.path().join("jobs"),
        p.data.path().join("offline-jobs"),
    )
    .unwrap();
    let mut rejected = profile.clone();
    rejected["outputs"] = json!({"../replacement.mkv":1});
    let rejected_path = p.data.path().join("refused-episode-profile.json");
    write(p.data.path(), "refused-episode-profile.json", &rejected);
    let refused = std::process::Command::new(&binary)
        .arg("run-episode")
        .arg(&rejected_path)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        p.owners
            .source(&key("bounded-episode-cli"))
            .unwrap()
            .is_none()
    );
    let output = std::process::Command::new(&binary)
        .arg("run-episode")
        .arg(&profile_path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostic = stderr
        .split("cache://sha256/")
        .nth(1)
        .and_then(|tail| tail.get(..64))
        .filter(|oid| oid.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|oid| {
            let file = fs::File::open(reel_assets::object(&p.hot(), oid)).ok()?;
            let mut raw = String::new();
            flate2::read::GzDecoder::new(file)
                .take(2 * 1024 * 1024 + 1)
                .read_to_string(&mut raw)
                .ok()?;
            let value: Value = serde_json::from_str(&raw).ok()?;
            Some(
                value["diagnostics"]["step-0.log"]["text"]
                    .as_str()
                    .unwrap_or("missing worker diagnostic")
                    .to_owned(),
            )
        })
        .unwrap_or_default();
    assert!(
        output.status.success(),
        "stderr={stderr}; diagnostic={diagnostic}; stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["approval"], false);
    assert_eq!(result["bindings_changed"], false);
    assert_eq!(result["production_claim"], Value::Null);
    assert_eq!(result["temporary_owner_cleanup_required"], false);
    assert_ne!(
        result["result"]["schema"], "reel.failed-job-result.v1",
        "{result}"
    );
    assert_eq!(result["result"]["workspace_retained"], false);
    let outputs = &result["result"]["outputs"];
    for name in ["native/master.mkv", "native/receipt.json", "worker.json"] {
        let item = &outputs[name];
        let observed =
            ByteIdentity::observe_file(Path::new(item["path"].as_str().unwrap())).unwrap();
        assert_eq!(observed.sha256, item["sha256"]);
        assert_eq!(observed.bytes, item["bytes"]);
    }
    let receipt: Value = serde_json::from_slice(
        &fs::read(outputs["native/receipt.json"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["total_frames"], 96);
    assert_eq!(receipt["decoded_master_matches_ordered_segments"], true);
    assert_eq!(receipt["timestamps_verified"], true);
    let journal_minimum = serde_json::from_value(result["journal_checkpoint"].clone()).unwrap();
    let mut journal = SourceJournal::open(&p.data.path().join("owners"), &journal_minimum).unwrap();
    let retained = journal
        .source(&key("bounded-episode-cli"))
        .unwrap()
        .unwrap();
    assert_eq!(retained.revision, 6);
    assert!(retained.pending.is_none());
    let mut reader = MetadataReader::open(&metadata, &minimum).unwrap();
    let kept =
        tig_core::tree::Tree::from_root(retained.current.as_ref().unwrap().selection.root.clone())
            .entries(&mut reader, TreeLimits::default())
            .unwrap()
            .0;
    let completion: Value = serde_json::from_slice(
        &fs::read(outputs["worker.json"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    let parent: SourceSelection = serde_json::from_value(completion["resources"].clone()).unwrap();
    let parent_nodes = tig_core::tree::Tree::from_root(parent.root.clone())
        .entries(&mut reader, TreeLimits::default())
        .unwrap()
        .0;
    assert!(kept.values().any(|object| object == &parent.root));
    assert!(
        parent_nodes
            .values()
            .all(|object| kept.values().any(|kept| kept == object))
    );
    for source in completion["temporary_sources"].as_array().unwrap() {
        let state = journal
            .source(&key(source.as_str().unwrap()))
            .unwrap()
            .unwrap();
        assert_eq!(state.revision, 3);
        assert!(state.current.is_none() && state.pending.is_none());
    }
    let checkpoint = journal.checkpoint();
    let finished = std::process::Command::new(&binary)
        .arg("finish-episode")
        .arg(&profile_path)
        .output()
        .unwrap();
    assert!(
        finished.status.success(),
        "{}",
        String::from_utf8_lossy(&finished.stderr)
    );
    let finished: Value = serde_json::from_slice(&finished.stdout).unwrap();
    assert_eq!(finished["temporary_owner_cleanup_required"], false);
    assert_eq!(
        finished["journal_checkpoint"],
        serde_json::to_value(checkpoint).unwrap()
    );
    // Normal repeated source revision is refused rather than starting a second
    // unadmitted render or overwriting completed output obligations.
    let retry = std::process::Command::new(&binary)
        .arg("run-episode")
        .arg(&profile_path)
        .output()
        .unwrap();
    assert!(!retry.status.success());
    assert!(String::from_utf8_lossy(&retry.stderr).contains("revision"));
    // An independently reused source must never be withdrawn by an old cleanup.
    let header = kept
        .get(&tig_core::LogicalPath::parse("handoff/identity").unwrap())
        .unwrap();
    let header: Value = serde_json::from_slice(&reader.get(header).unwrap().unwrap()).unwrap();
    let old = &header["temporary"][0];
    let source = key(old["source"].as_str().unwrap());
    let selected: SourceSelection = serde_json::from_value(old["selection"].clone()).unwrap();
    let prepared = journal
        .apply(
            key("reuse-temporary-prepare"),
            tig_store::source_claims::Command::Prepare {
                source: source.clone(),
                owner: key("fixture"),
                expected_revision: 3,
                selection: selected,
            },
        )
        .unwrap();
    journal
        .apply(
            key("reuse-temporary-adopt"),
            tig_store::source_claims::Command::Adopt {
                source: source.clone(),
                owner: key("fixture"),
                expected_revision: prepared.source_revision,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    let stale = std::process::Command::new(&binary)
        .arg("finish-episode")
        .arg(&profile_path)
        .output()
        .unwrap();
    assert!(!stale.status.success());
    assert_eq!(journal.source(&source).unwrap().unwrap().revision, 5);
    assert!(journal.source(&source).unwrap().unwrap().current.is_some());
}
