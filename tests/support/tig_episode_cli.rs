//! Real CLI -> admitted process tree -> native reference proofs -> retained movie.
use super::*;
use reel::media_runtime::ByteIdentity;
use std::io::Read;
use tig_core::tree::NodeStore;
use tig_store::source_claims::Selection as SourceSelection;
#[path = "tig_mixed_episode_cli.rs"]
mod mixed_components;
#[path = "tig_episode_records.rs"]
mod output_records;

#[test]
#[ignore = "requires native FFmpeg; bounded episode CLI with captured source and old jobs offline"]
fn bounded_episode_cli_reopens_source_verifies_components_and_retains_checked_movie() {
    exercise(false);
}
#[test]
#[ignore = "requires native FFmpeg; mixed scene/presentation episode with source checkout offline"]
fn mixed_episode_cli_checks_opening_scenes_and_credits_with_complete_owned_handoff() {
    exercise(true);
}
#[test]
#[ignore = "requires native FFmpeg to establish retained synthetic scene fixtures"]
fn prepare_episode_freezes_child_resources_with_movie_and_source_journal_unavailable() {
    exercise_mode(false, true);
}
#[test]
#[ignore = "requires native FFmpeg to establish retained mixed-presentation fixtures"]
fn prepare_mixed_episode_freezes_complete_resources_without_execution_admission() {
    exercise_mode(true, true);
}
#[test]
#[ignore = "requires native FFmpeg; source-owned component verification and recovery"]
fn verify_episode_components_retains_proofs_without_assembling_or_emitting_records() {
    exercise_purpose(false, false, true);
}
#[test]
#[ignore = "requires native FFmpeg; mixed component verification without assembly"]
fn verify_mixed_episode_components_checks_primaries_and_cleans_owned_sources() {
    exercise_purpose(true, false, true);
}
fn check_verification(p: &Project, binary: &Path, profile: &Value, path: &Path, scenes: &[Scene]) {
    let wrong = Command::new(binary)
        .arg("run-episode")
        .arg(path)
        .output()
        .unwrap();
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("purpose differs"));
    let run = Command::new(binary)
        .arg("verify-episode")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(result["succeeded"], true);
    assert_eq!(result["purpose"], "verify-components");
    assert_eq!(result["temporary_owner_cleanup_required"], false);
    assert_eq!(result["result"]["outputs"].as_object().unwrap().len(), 1);
    let report = read(
        result["result"]["outputs"]["worker.json"]["path"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(report["components_verified"], 4);
    assert_eq!(report["component_primary_references_verified"], true);
    assert_eq!(report["episode_assembled"], false);
    assert_eq!(report["final_episode_output_verified"], false);
    assert!(report["production_claim"].is_null());
    assert_eq!(report["approval"], false);
    let minimum = serde_json::from_value(result["journal_checkpoint"].clone()).unwrap();
    let mut journal = SourceJournal::open(&p.data.path().join("owners"), &minimum).unwrap();
    for id in report["temporary_sources"].as_array().unwrap() {
        let state = journal.source(&key(id.as_str().unwrap())).unwrap().unwrap();
        assert!(state.current.is_none() && state.pending.is_none());
    }
    let checkpoint = journal.checkpoint();
    let finish = Command::new(binary)
        .arg("finish-episode")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        finish.status.success(),
        "{}",
        String::from_utf8_lossy(&finish.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&finish.stdout).unwrap()["journal_checkpoint"],
        json!(checkpoint)
    );
    let mut changed = profile.clone();
    changed["inputs"]["purpose"] = json!("assemble");
    let changed_path = p.data.path().join("changed-purpose.json");
    write(p.data.path(), "changed-purpose.json", &changed);
    let refused = Command::new(binary)
        .arg("finish-episode")
        .arg(changed_path)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(journal.checkpoint(), checkpoint);

    // A missing primary reaches the contained checker, fails, and retains failure
    // diagnostics before releasing temporary sources. It must not become success.
    let missing = scenes[1].primary.iter().next().unwrap();
    let original = object_file(&p.hot(), missing);
    let backup = p.data.path().join("verification-primary-offline");
    fs::rename(&original, &backup).unwrap();
    let mut failed = profile.clone();
    failed["source_id"] = json!("bounded-component-verification-failure");
    failed["operation"] = json!("verify-fail-v1");
    let failed_path = p.data.path().join("failed-component-verification.json");
    write(p.data.path(), "failed-component-verification.json", &failed);
    let run = Command::new(binary)
        .arg("verify-episode")
        .arg(&failed_path)
        .output()
        .unwrap();
    fs::rename(&backup, &original).unwrap();
    assert!(!run.status.success());
    let failed: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(failed["succeeded"], false);
    assert_eq!(failed["handoff"]["succeeded"], false);
    assert_eq!(failed["temporary_owner_cleanup_required"], false);
    let finish = Command::new(binary)
        .arg("finish-episode")
        .arg(failed_path)
        .output()
        .unwrap();
    assert!(finish.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&finish.stdout).unwrap()["succeeded"],
        false
    );
}
fn exercise(mixed: bool) {
    exercise_mode(mixed, false);
}
fn exercise_mode(mixed: bool, preparation_only: bool) {
    exercise_purpose(mixed, preparation_only, false);
}
fn exercise_purpose(mixed: bool, preparation_only: bool, verification_only: bool) {
    if !preparation_only && !verification_only {
        assert!(
            std::env::var_os("REEL_TEST_TIG_PROPOSE").is_some(),
            "explicit pinned Tig proposal executable required before rendering fixtures"
        );
    }
    let mut p = Project::new();
    let scenes = (0..4)
        .map(|i| p.render_delivery(i, "cli-v1"))
        .collect::<Vec<_>>();
    p.assemble(&scenes, "cli-v1");
    let mut request = add_episode_assembly(&mut p, "episode-cli-v1.json");
    let _parent_controls = parent_controls::exercise(&p, &request);
    let mut presentations = if mixed {
        mixed_components::configure(&mut p, &mut request)
    } else {
        Vec::new()
    };
    // Logical output IDs are not filenames. Exercise the production namespace
    // through capture, independent comparisons, parent assembly and handoff.
    let mut assembly = read(p.repo.path().join("records/assembly.tig"));
    for id in UNITS {
        let name = format!("records/output-{id}.tig");
        let mut output = read(p.repo.path().join(&name));
        let logical = format!("{id}:master:es");
        output["id"] = json!(logical);
        write(p.repo.path(), &name, &output);
        assembly["authored"]["components"][id] = json!({"id":logical});
        for presentation in &mut presentations {
            if presentation["id"] == id {
                presentation["output_id"] = json!(logical);
            }
        }
    }
    write(p.repo.path(), "records/assembly.tig", &assembly);
    let record_fixture = output_records::configure(&mut p);
    p.commit("use opaque language-scoped component IDs");
    let git_base = git(p.repo.path(), &["rev-parse", "HEAD"]);
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
    let minimum = writer.finish().unwrap();
    drop(writer);
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_reel-tig-job"))
        .canonicalize()
        .unwrap();
    let mut runtime = serde_json::to_value(&p.runtime).unwrap();
    runtime["host"] = serde_json::to_value(ByteIdentity::observe_file(&binary).unwrap()).unwrap();
    let config = json!({"path_policy":"configured-local-roots","cache_roots":[p.hot()],"coordination_root":p.data.path().join("coordination"),"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}});
    let capture = json!({"schema":"reel.tig-episode-capture-profile.experimental-1","source":p.profile,"request":request,
        "metadata_root":metadata,"metadata_minimum":minimum,"metadata_cache":p.hot(),"preparation_cache":p.hot(),"cache":config});
    write(p.data.path(), "episode-capture-profile.json", &capture);
    let fragment_path = p.data.path().join("episode-context.json");
    let captured = std::process::Command::new(&binary)
        .arg("capture-context")
        .arg(p.data.path().join("episode-capture-profile.json"))
        .arg(&fragment_path)
        .output()
        .unwrap();
    assert!(
        captured.status.success(),
        "{}",
        String::from_utf8_lossy(&captured.stderr)
    );
    let summary: Value = serde_json::from_slice(&captured.stdout).unwrap();
    assert_eq!(summary["context_kind"], "episode");
    assert_eq!(summary["readback_equal"], true);
    assert_eq!(summary["media_rendered"], false);
    assert_eq!(summary["job_adopted"], false);
    assert!(
        p.owners
            .source(&key("bounded-episode-cli"))
            .unwrap()
            .is_none()
    );
    let fragment = read(&fragment_path);
    let original = fs::read(&fragment_path).unwrap();
    let repeated = std::process::Command::new(&binary)
        .arg("capture-context")
        .arg(p.data.path().join("episode-capture-profile.json"))
        .arg(&fragment_path)
        .output()
        .unwrap();
    assert!(!repeated.status.success());
    assert_eq!(fs::read(&fragment_path).unwrap(), original);
    let minimum: tig_store::metadata::Checkpoint =
        serde_json::from_value(fragment["metadata_minimum"].clone()).unwrap();
    let mut inputs = json!({"source":fragment["source"],"context":fragment["context"],"request":fragment["request"],"runtime":runtime,
        "record_outputs":record_fixture.specs,"git_handoff":record_fixture.handoff(&p, &git_base),
        "scenes":p.requests.iter().zip(UNITS).filter(|(_,id)|!mixed || matches!(*id,"scene-a"|"scene-b")).map(|(request,id)|json!({"id":id,"request":request,"output_id":format!("{id}:master:es"),"output_definition":"types/output","compiled":id=="scene-b"})).collect::<Vec<_>>(),"presentations":presentations});
    if verification_only {
        inputs["purpose"] = json!("verify-components");
        inputs["record_outputs"] = json!([]);
        inputs["git_handoff"] = Value::Null;
    }
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
    let missing: Object = serde_json::from_value(
        read(p.repo.path().join("offline-records/output-scene-b.tig"))["selected"].clone(),
    )
    .unwrap();
    let missing_path = object_file(&p.hot(), &missing);
    let backup = p.data.path().join("component-master-offline");
    fs::rename(&missing_path, &backup).unwrap();
    // Planning must remain possible with source Git and a selected movie offline.
    // It must not even open/create the source journal or pretend media is verified.
    let mut preparation = profile.clone();
    let absent_journal = p.data.path().join("preparation-must-not-create-journal");
    preparation["journal_root"] = json!(absent_journal);
    let preparation_profile = p.data.path().join("episode-preparation-profile.json");
    write(
        p.data.path(),
        "episode-preparation-profile.json",
        &preparation,
    );
    let report_path = p.data.path().join("episode-preparation-report.json");
    let checkpoint_before = p.owners.checkpoint();
    let forbidden = p
        .hot()
        .join("objects")
        .join("..")
        .join("objects")
        .join("not-a-report");
    let refused = Command::new(&binary)
        .arg("prepare-episode")
        .arg(&preparation_profile)
        .arg(&forbidden)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(!forbidden.exists());
    let stream_path = p.data.path().join("preparation-report.json:stream");
    let refused = Command::new(&binary)
        .arg("prepare-episode")
        .arg(&preparation_profile)
        .arg(&stream_path)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(!stream_path.exists());
    let prepared = Command::new(&binary)
        .arg("prepare-episode")
        .arg(&preparation_profile)
        .arg(&report_path)
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let summary: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(summary["components"], 4);
    assert_eq!(summary["readback_equal"], true);
    for field in [
        "component_payloads_verified",
        "production_currency_verified",
        "job_adopted",
        "execution_capacity_reserved",
        "media_rendered",
        "authorizes_production",
        "authorizes_eviction",
    ] {
        assert_eq!(summary[field], false, "{field}");
    }
    assert!(summary["resource_objects"].as_u64().unwrap() > 4);
    let prepared_report = read(&report_path);
    let resources: SourceSelection =
        serde_json::from_value(prepared_report["resources"].clone()).unwrap();
    let minimum = serde_json::from_value(prepared_report["metadata_minimum"].clone()).unwrap();
    let mut resource_reader = MetadataReader::open(&metadata, &minimum).unwrap();
    let (entries, _) = tig_core::tree::Tree::from_root(resources.root)
        .entries(&mut resource_reader, TreeLimits::default())
        .unwrap();
    let objects = entries
        .values()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        objects.contains(&missing),
        "composition bytes must remain declared even when unavailable"
    );
    for primary in scenes
        .iter()
        .filter(|scene| !mixed || matches!(scene.unit.as_str(), "scene-a" | "scene-b"))
        .flat_map(|scene| &scene.primary)
    {
        assert!(
            objects.contains(primary),
            "child primary input missing from complete resource plan"
        );
    }
    drop(resource_reader);
    assert_eq!(
        prepared_report["profile"],
        json!(Object::from_bytes(&fs::read(&preparation_profile).unwrap()))
    );
    assert_eq!(prepared_report["root_retention_enrolled"], false);
    assert!(!absent_journal.exists());
    assert_eq!(p.owners.checkpoint(), checkpoint_before);
    assert!(!missing_path.exists());
    let report_bytes = fs::read(&report_path).unwrap();
    let repeated = Command::new(&binary)
        .arg("prepare-episode")
        .arg(&preparation_profile)
        .arg(&report_path)
        .output()
        .unwrap();
    assert!(!repeated.status.success());
    assert_eq!(fs::read(&report_path).unwrap(), report_bytes);
    preparation["max_objects"] = json!(1);
    write(
        p.data.path(),
        "episode-preparation-profile.json",
        &preparation,
    );
    let refused_report = p.data.path().join("refused-preparation-report.json");
    let refused = Command::new(&binary)
        .arg("prepare-episode")
        .arg(&preparation_profile)
        .arg(&refused_report)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("complete input resource budget exceeded")
    );
    assert!(!refused_report.exists());
    assert!(!absent_journal.exists());
    assert_eq!(p.owners.checkpoint(), checkpoint_before);
    if preparation_only {
        fs::rename(&backup, &missing_path).unwrap();
        return;
    }
    let denied = Command::new(&binary)
        .arg(if verification_only {
            "verify-episode"
        } else {
            "run-episode"
        })
        .arg(&profile_path)
        .output()
        .unwrap();
    fs::rename(backup, missing_path).unwrap();
    assert!(!denied.status.success());
    let error = String::from_utf8_lossy(&denied.stderr);
    assert!(
        error.contains("episode component scene-b selected object"),
        "{error}"
    );
    assert!(error.contains(missing.sha256.as_str()), "{error}");
    assert!(
        p.owners
            .source(&key("bounded-episode-cli"))
            .unwrap()
            .is_none()
    );
    if verification_only {
        check_verification(&p, &binary, &profile, &profile_path, &scenes);
        return;
    }
    if mixed {
        for unit in ["opening", "credits"] {
            fs::rename(
                p.data.path().join(format!("mixed-original-{unit}")),
                p.data.path().join(format!("offline-mixed-{unit}")),
            )
            .unwrap();
        }
    }
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
    let mut undersized = profile.clone();
    undersized["cache"]["capacity"]["max_object_bytes"] = json!(128 * 1024 * 1024);
    undersized["inputs"]["source"] = json!({}); // Must refuse before reading this invalid archive.
    let path = p.data.path().join("undersized-episode-policy.json");
    write(p.data.path(), "undersized-episode-policy.json", &undersized);
    let denied = std::process::Command::new(&binary)
        .arg("run-episode")
        .arg(path)
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(
        String::from_utf8_lossy(&denied.stderr)
            .contains("workspace budget exceeds configured operation allowance")
    );
    assert!(
        p.owners
            .source(&key("bounded-episode-cli"))
            .unwrap()
            .is_none()
    );
    let mut wrong_template = profile.clone();
    wrong_template["inputs"]["record_outputs"][0]["template"]["object"] =
        json!(Object::from_bytes(b"changed template"));
    let bad_path = p.data.path().join("wrong-record-template.json");
    write(p.data.path(), "wrong-record-template.json", &wrong_template);
    let denied = Command::new(&binary)
        .arg("run-episode")
        .arg(bad_path)
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(
        String::from_utf8_lossy(&denied.stderr).contains("template or definition is not admitted")
    );
    assert!(
        p.owners
            .source(&key("bounded-episode-cli"))
            .unwrap()
            .is_none()
    );
    if mixed {
        let mut incomplete = profile.clone();
        incomplete["inputs"]["presentations"]
            .as_array_mut()
            .unwrap()
            .pop();
        let path = p.data.path().join("incomplete-mixed-profile.json");
        write(p.data.path(), "incomplete-mixed-profile.json", &incomplete);
        let refused = std::process::Command::new(&binary)
            .arg("run-episode")
            .arg(path)
            .output()
            .unwrap();
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("complete component coverage"));
        assert!(
            p.owners
                .source(&key("bounded-episode-cli"))
                .unwrap()
                .is_none()
        );
    }
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
    let segments = receipt["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 4);
    for (index, segment) in segments.iter().enumerate() {
        let presentation = mixed && matches!(index, 0 | 3);
        assert_eq!(segment["upstream_presentation_verified"], presentation);
        assert_eq!(segment["upstream_delivery_verified"], !presentation);
    }
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
        serde_json::to_value(&checkpoint).unwrap()
    );
    let mut changed_contract = profile.clone();
    changed_contract["inputs"]["record_outputs"] = json!([]);
    changed_contract["inputs"]["git_handoff"] = Value::Null;
    let changed_path = p.data.path().join("changed-record-contract.json");
    write(
        p.data.path(),
        "changed-record-contract.json",
        &changed_contract,
    );
    let refused = Command::new(&binary)
        .arg("finish-episode")
        .arg(changed_path)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("another attempt"));
    let candidate_path = outputs["record-candidates.json"]["path"].as_str().unwrap();
    let original_index = fs::read(candidate_path).unwrap();
    let mut corrupted = original_index.clone();
    corrupted[0] ^= 1;
    fs::write(candidate_path, corrupted).unwrap();
    let refused = Command::new(&binary)
        .arg("finish-episode")
        .arg(&profile_path)
        .output()
        .unwrap();
    fs::write(candidate_path, original_index).unwrap();
    assert!(!refused.status.success());
    assert_eq!(journal.checkpoint(), checkpoint);
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

    // Oversized failed output cannot be salvaged: staging and all claims stay
    // live. A refused retirement must never look like completed cleanup.
    let mut failed_profile = profile.clone();
    failed_profile["source_id"] = json!("bounded-episode-cli-unretained");
    failed_profile["outputs"]["receipt.json"] = json!(1);
    let before_sources = journal
        .obligations(1024)
        .unwrap()
        .iter()
        .map(|item| item.source.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    write(
        p.data.path(),
        "bounded-episode-unretained-profile.json",
        &failed_profile,
    );
    let unsafe_failure = std::process::Command::new(&binary)
        .arg("run-episode")
        .arg(
            p.data
                .path()
                .join("bounded-episode-unretained-profile.json"),
        )
        .output()
        .unwrap();
    assert!(!unsafe_failure.status.success());
    assert!(unsafe_failure.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&unsafe_failure.stderr)
            .contains("staging preserved because retirement verification failed")
    );
    let state = journal
        .source(&key("bounded-episode-cli-unretained"))
        .unwrap()
        .unwrap();
    assert_eq!(state.revision, 2);
    assert!(state.current.is_some());
    let new_sources = journal
        .obligations(1024)
        .unwrap()
        .iter()
        .map(|item| item.source.as_str().to_owned())
        .filter(|source| !before_sources.contains(source))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        new_sources.len(),
        6,
        "root and all five worker claims must remain live"
    );
    for source in new_sources {
        let state = journal.source(&key(&source)).unwrap().unwrap();
        assert_eq!(state.revision, 2);
        assert!(state.current.is_some() && state.pending.is_none());
    }

    // Fail after component ownership, before writing outputs, by admitting a
    // valid but insufficient primary-reference staging budget. The runner
    // retains this failure's diagnostics before returning an error.
    let mut failed_profile = profile.clone();
    failed_profile["source_id"] = json!("bounded-episode-cli-failed");
    failed_profile["reference_staging_bytes"] = json!(1);
    write(
        p.data.path(),
        "bounded-episode-failed-profile.json",
        &failed_profile,
    );
    let failed_path = p.data.path().join("bounded-episode-failed-profile.json");
    let failed = std::process::Command::new(&binary)
        .arg("run-episode")
        .arg(&failed_path)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let failed: Value = serde_json::from_slice(&failed.stdout).unwrap_or_else(|_| {
        panic!(
            "failed CLI did not retain/clean its context: {}",
            String::from_utf8_lossy(&failed.stderr)
        )
    });
    assert_eq!(failed["succeeded"], false);
    assert_eq!(failed["result"]["schema"], "reel.failed-job-result.v1");
    assert_eq!(failed["handoff"]["succeeded"], false);
    assert_eq!(failed["temporary_owner_cleanup_required"], false);
    let state = journal
        .source(&key("bounded-episode-cli-failed"))
        .unwrap()
        .unwrap();
    assert_eq!(state.revision, 6);
    let retained = tig_core::tree::Tree::from_root(state.current.unwrap().selection.root)
        .entries(&mut reader, TreeLimits::default())
        .unwrap()
        .0;
    let object = retained
        .get(&tig_core::LogicalPath::parse("handoff/identity").unwrap())
        .unwrap();
    let packet: Value = serde_json::from_slice(&reader.get(object).unwrap().unwrap()).unwrap();
    assert_eq!(packet["succeeded"], false);
    assert_eq!(packet["temporary"].as_array().unwrap().len(), 4);
    let prior: SourceSelection = serde_json::from_value(packet["previous"].clone()).unwrap();
    let prior_nodes = tig_core::tree::Tree::from_root(prior.root.clone())
        .entries(&mut reader, TreeLimits::default())
        .unwrap()
        .0;
    assert!(
        prior_nodes
            .values()
            .chain(std::iter::once(&prior.root))
            .all(|object| retained.values().any(|kept| kept == object))
    );
    for name in ["result/request", "result/evidence"] {
        let object = prior_nodes
            .get(&tig_core::LogicalPath::parse(name).unwrap())
            .unwrap();
        assert!(retained.values().any(|kept| kept == object));
        let path = object_file(&p.hot(), object);
        let reopened = ByteIdentity::observe_file(&path).unwrap();
        assert_eq!(reopened.sha256, object.sha256.as_str());
        assert_eq!(reopened.bytes, object.bytes);
        if name == "result/request" {
            let request = read(&path);
            assert_eq!(
                request["step_timeout_limit_seconds"],
                profile["timeout_seconds"]
            );
            assert_eq!(
                request["steps"][0]["timeout_seconds"],
                profile["timeout_seconds"]
            );
        }
    }
    for item in packet["temporary"].as_array().unwrap() {
        let state = journal
            .source(&key(item["source"].as_str().unwrap()))
            .unwrap()
            .unwrap();
        assert_eq!(state.revision, 3);
        assert!(state.current.is_none() && state.pending.is_none());
    }
    let checkpoint = journal.checkpoint();
    let finish = std::process::Command::new(&binary)
        .arg("finish-episode")
        .arg(&failed_path)
        .output()
        .unwrap();
    assert!(
        finish.status.success(),
        "{}",
        String::from_utf8_lossy(&finish.stderr)
    );
    let finish: Value = serde_json::from_slice(&finish.stdout).unwrap();
    assert_eq!(finish["succeeded"], false);
    assert_eq!(
        finish["journal_checkpoint"],
        serde_json::to_value(checkpoint).unwrap()
    );
    output_records::check(&p, &result, &git_base, &record_fixture);
}
