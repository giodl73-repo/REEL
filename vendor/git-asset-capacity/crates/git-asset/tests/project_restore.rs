#![cfg(feature = "assets")]
mod common;
use common::{git, git_executable};
use git_asset::project;
use git_asset_core::{Digest, Object};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const ALPHA: &[u8] = b"synthetic picture";
const BETA: &[u8] = b"synthetic cue bytes";

const PRIVATE_A: &str = ".git-asset-restore-00000000000000000000000000000000.partial";
const PRIVATE_B: &str = ".git-asset-restore-11111111111111111111111111111111.partial";

#[cfg(any(unix, windows))]
fn directory_link(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(any(unix, windows))]
#[test]
fn partial_inspection_refuses_linked_ancestry_without_listing_the_target() {
    let f = Fixture::new();
    let outside = f.temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join(PRIVATE_A), ALPHA).unwrap();
    fs::create_dir_all(&f.destination).unwrap();
    directory_link(&f.destination.join("inputs"), &outside);
    let plan = f.plan("profiles/nested/render.json");
    assert!(plan.inspect_partials(plan.identity()).is_err());
    assert_eq!(fs::read(outside.join(PRIVATE_A)).unwrap(), ALPHA);
}

#[cfg(any(unix, windows))]
#[test]
fn partial_inspection_never_follows_matching_link_entries() {
    let f = Fixture::new();
    let outside = f.temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("unrelated"), ALPHA).unwrap();
    let directory = f.destination.join("inputs");
    fs::create_dir_all(&directory).unwrap();
    directory_link(&directory.join(PRIVATE_A), &outside);
    let plan = f.plan("profiles/nested/render.json");
    let report = plan.inspect_partials(plan.identity()).unwrap();
    assert_eq!(report.scanned_entries, 1);
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].classification, "unverified-entry");
    assert_eq!(report.candidates[0].observed_logical_bytes, None);
    assert_eq!(report.observed_partial_logical_bytes, 0);
    assert_eq!(fs::read(outside.join("unrelated")).unwrap(), ALPHA);
}

#[test]
fn partial_inspection_missing_parents_never_initializes_or_reads_payloads() {
    let f = Fixture::new();
    fs::remove_file(
        f.store
            .join("objects")
            .join(Object::from_bytes(ALPHA).sha256.as_str()),
    )
    .unwrap();
    let plan = f.plan("profiles/nested/render.json");
    let report = plan.inspect_partials(plan.identity()).unwrap();
    assert_eq!(report.requested_directories, 1);
    assert_eq!(report.missing_directories, 1);
    assert_eq!(report.scanned_entries, 0);
    assert!(report.candidates.is_empty());
    assert!(!f.destination.exists());
    assert!(!report.source_payloads_read);
    assert!(!report.authorizes_cleanup);
}

#[test]
fn partial_inspection_is_nonrecursive_bounded_sorted_and_preserves_all_bytes() {
    let f = Fixture::new();
    let directory = f.destination.join("inputs");
    fs::create_dir_all(directory.join("unplanned")).unwrap();
    fs::write(directory.join(PRIVATE_B), BETA).unwrap();
    fs::write(directory.join(PRIVATE_A), []).unwrap();
    fs::write(directory.join("unrelated.txt"), ALPHA).unwrap();
    fs::write(directory.join("unplanned").join(PRIVATE_A), ALPHA).unwrap();
    fs::write(f.temp.path().join(PRIVATE_A), ALPHA).unwrap();
    let plan = f.plan("profiles/nested/render.json");
    let report = plan.inspect_partials(plan.identity()).unwrap();
    assert_eq!(report.requested_directories, 1);
    assert_eq!(report.scanned_entries, 4);
    assert_eq!(report.candidates.len(), 2);
    assert!(report.candidates[0].path.as_str().ends_with(PRIVATE_A));
    assert!(report.candidates[1].path.as_str().ends_with(PRIVATE_B));
    assert_eq!(report.observed_partial_logical_bytes, BETA.len() as u64);
    assert_eq!(report.candidates[0].observed_logical_bytes, Some(0));
    assert!(
        report
            .candidates
            .iter()
            .all(|c| c.classification == "unassociated-private-partial")
    );
    assert_eq!(fs::read(directory.join(PRIVATE_B)).unwrap(), BETA);
    assert_eq!(
        fs::read(directory.join("unplanned").join(PRIVATE_A)).unwrap(),
        ALPHA
    );
    assert_eq!(report.physical_reclaimable_bytes, None);
    let value = serde_json::to_value(report).unwrap();
    for flag in [
        "atomic_snapshot",
        "source_payloads_read",
        "destination_payloads_verified",
        "ownership_verified",
        "active_writers_known",
        "authorizes_cleanup",
        "authorizes_eviction",
    ] {
        assert_eq!(value[flag], false);
    }
}

#[test]
fn partial_inspection_counts_unrelated_entries_and_never_truncates_silently() {
    let mut f = Fixture::new();
    let directory = f.destination.join("inputs");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("ordinary-a"), ALPHA).unwrap();
    fs::write(directory.join("ordinary-b"), BETA).unwrap();
    f.profile["limits"]["partial_directory_entries"] = json!(1);
    let plan = f.plan("profiles/nested/render.json");
    assert!(plan.inspect_partials(plan.identity()).is_err());
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
    for limit in [0, 16385] {
        f.profile["limits"]["partial_directory_entries"] = json!(limit);
        assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
    }
    f.profile["limits"]["partial_directory_entries"] = json!(16384);
    let plan = f.plan("profiles/nested/render.json");
    assert_eq!(
        plan.inspect_partials(plan.identity())
            .unwrap()
            .scanned_entries,
        2
    );
}

#[test]
fn partial_inspection_does_not_infer_ownership_or_liveness_from_names() {
    use std::io::Write;
    let f = Fixture::new();
    let directory = f.destination.join("inputs");
    fs::create_dir_all(&directory).unwrap();
    let mut active = fs::File::create(directory.join(PRIVATE_A)).unwrap();
    active.write_all(ALPHA).unwrap();
    active.flush().unwrap();
    fs::create_dir(directory.join(PRIVATE_B)).unwrap();
    fs::write(
        directory.join(".git-asset-restore-not-a-nonce.partial"),
        BETA,
    )
    .unwrap();
    let plan = f.plan("profiles/nested/render.json");
    let report = plan.inspect_partials(plan.identity()).unwrap();
    assert_eq!(report.scanned_entries, 3);
    assert_eq!(report.candidates.len(), 2);
    assert!(!report.active_writers_known);
    assert!(!report.ownership_verified);
    assert_eq!(report.candidates[1].classification, "non-file-refused");
    assert_eq!(report.candidates[1].observed_logical_bytes, None);
    active.write_all(BETA).unwrap();
    drop(active);
    assert_eq!(
        fs::metadata(directory.join(PRIVATE_A)).unwrap().len(),
        (ALPHA.len() + BETA.len()) as u64
    );
}

#[test]
fn partial_inspection_refuses_stale_plan_and_non_directory_parent() {
    let f = Fixture::new();
    fs::create_dir_all(&f.destination).unwrap();
    fs::write(f.destination.join("inputs"), ALPHA).unwrap();
    let plan = f.plan("profiles/nested/render.json");
    assert_eq!(
        plan.inspect_partials(&Digest::blob(b"stale"))
            .err()
            .unwrap(),
        "expected plan differs"
    );
    assert!(plan.inspect_partials(plan.identity()).is_err());
    assert_eq!(fs::read(f.destination.join("inputs")).unwrap(), ALPHA);
}

#[test]
fn partial_inspection_after_interruption_and_retry_never_selects_or_cleans_candidates() {
    let f = Fixture::new();
    let plan = f.plan("profiles/nested/render.json");
    let partial = plan.run_observed(true, None, |_| false).unwrap();
    assert!(partial.failure.is_some());
    let leftover = f.destination.join("inputs").join(PRIVATE_A);
    fs::write(&leftover, b"synthetic interrupted private write").unwrap();
    let before = plan.inspect_partials(plan.identity()).unwrap();
    assert_eq!(before.candidates.len(), 1);
    let retry = plan.run(true, Some(plan.identity())).unwrap();
    assert!(retry.failure.is_none());
    assert_eq!(retry.entries[0].action, "already-present");
    let after = plan.inspect_partials(plan.identity()).unwrap();
    assert_eq!(after.candidates.len(), 1);
    assert_eq!(before.candidates[0].path, after.candidates[0].path);
    assert_eq!(
        fs::read(&leftover).unwrap(),
        b"synthetic interrupted private write"
    );
}

#[test]
fn partial_inspection_cli_requires_exact_plan_and_excludes_other_operation_modes() {
    let f = Fixture::new();
    let plan = f.plan("profiles/nested/render.json");
    let expected = plan.identity().as_str();
    let out = f.run(&["--inspect-partials", "--expect-plan", expected]);
    assert!(out.status.success());
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["schema"],
        "tig.project-partial-inspection.experimental-1"
    );
    assert_eq!(report["missing_directories"], 1);
    for flags in [
        vec!["--inspect-partials"],
        vec![
            "--inspect-partials",
            "--inspect-partials",
            "--expect-plan",
            expected,
        ],
        vec!["--inspect-partials", "--apply", "--expect-plan", expected],
        vec![
            "--inspect-partials",
            "--progress",
            "unused",
            "--expect-plan",
            expected,
        ],
        vec![
            "--inspect-partials",
            "--inspect-progress",
            "unused",
            "--expect-plan",
            expected,
        ],
    ] {
        assert!(!f.run(&flags).status.success());
    }
    assert!(!f.destination.exists());
}

#[test]
fn handoff_preserves_alias_selectors_and_unverified_consumer_links_without_extra_io() {
    let mut f = Fixture::new();
    let absent = Object::from_bytes(b"external runtime evidence not enrolled in this cache");
    f.profile["consumer_evidence"] = json!({"runtime/build":absent,"recipe":absent});
    let plan = f.plan("profiles/nested/render.json");
    let preview = plan.run(false, None).unwrap();
    assert!(preview.failure.is_none());
    assert_eq!(preview.source_verifications, 2);
    assert_eq!(preview.unique_objects, 2);
    assert_eq!(preview.consumer_evidence.len(), 2);
    assert!(!preview.consumer_evidence_verified);
    assert!(!f.destination.exists());
    let result = serde_json::to_value(plan.run(true, Some(plan.identity())).unwrap()).unwrap();
    assert_eq!(
        result["schema"],
        "tig.project-restore-result.experimental-4"
    );
    assert_eq!(result["entries"][1]["selector"], json!({"kind":"selected"}));
    assert_eq!(
        result["entries"][2]["selector"],
        json!({"kind":"stream","slot":"selected"})
    );
    assert_eq!(result["entries"][2]["destination"], "inputs/alias.bin");
    assert_eq!(
        result["entries"][0]["object"],
        result["entries"][2]["object"]
    );
    assert_eq!(result["consumer_evidence"]["runtime/build"], json!(absent));
    assert_eq!(result["source_verifications"], 5);
    assert_eq!(result["consumer_evidence_verified"], false);
    for flag in [
        "source_guards_held",
        "independent_custody_verified",
        "authorizes_eviction",
        "complete_semantic_coverage",
    ] {
        assert_eq!(result[flag], false);
    }
}

#[test]
fn consumer_evidence_changes_fence_the_plan_without_changing_byte_selections() {
    let mut f = Fixture::new();
    let old = f.plan("profiles/nested/render.json");
    f.profile["consumer_evidence"] = json!({"runtime":Object::from_bytes(b"build one")});
    let first = f.plan("profiles/nested/render.json");
    assert_ne!(old.identity(), first.identity());
    f.profile["consumer_evidence"]["runtime"] = json!(Object::from_bytes(b"build two"));
    let second = f.plan("profiles/nested/render.json");
    assert_ne!(first.identity(), second.identity());
    assert!(second.run(true, Some(first.identity())).is_err());
    assert!(!f.destination.exists());
    let before = old.run(false, None).unwrap();
    let after = second.run(false, None).unwrap();
    assert_eq!(before.unique_object_bytes, after.unique_object_bytes);
    for (a, b) in before.entries.iter().zip(after.entries.iter()) {
        assert_eq!(a.object, b.object);
        assert_eq!(a.record, b.record);
    }
}

#[test]
fn consumer_evidence_strict_bounds_labels_and_objects_refuse_before_writes() {
    let mut f = Fixture::new();
    let mut links = serde_json::Map::new();
    for n in 0..16 {
        links.insert(
            format!("evidence/{n}"),
            json!(Object::from_bytes(b"same bytes")),
        );
    }
    f.profile["consumer_evidence"] = Value::Object(links.clone());
    assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_ok());
    links.insert(
        "evidence/overflow".into(),
        json!(Object::from_bytes(b"same bytes")),
    );
    f.profile["consumer_evidence"] = Value::Object(links);
    assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
    for label in [
        "",
        "../runtime",
        "C:/runtime",
        "/runtime",
        "runtime\\build",
        "runtime\n",
        &"x".repeat(129),
    ] {
        f.profile["consumer_evidence"] = json!({label:Object::from_bytes(b"pin")});
        assert!(
            project::prepare(&f.raw(), "profiles/nested/render.json").is_err(),
            "{label:?}"
        );
    }
    f.profile["consumer_evidence"] = json!({"x".repeat(128):Object::from_bytes(b"pin")});
    assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_ok());
    for bad in [
        Value::Null,
        json!([]),
        json!({"runtime":null}),
        json!({"runtime":{"sha256":"bad","bytes":1}}),
        json!({"runtime":{"sha256":Object::from_bytes(b"pin").sha256,"bytes":1,"path":"ignored"}}),
    ] {
        f.profile["consumer_evidence"] = bad;
        assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
    }
    assert!(!f.destination.exists());
}

#[test]
fn failed_and_interrupted_handoffs_keep_links_but_do_not_claim_completion() {
    let mut f = Fixture::new();
    f.profile["consumer_evidence"] = json!({"review":Object::from_bytes(b"uninterpreted review")});
    let plan = f.plan("profiles/nested/render.json");
    let partial = plan.run_observed(true, None, |_| false).unwrap();
    assert_eq!(partial.failure.unwrap().phase, "interrupted");
    assert_eq!(partial.entries[0].action, "restored");
    assert_eq!(partial.entries[1].action, "pending");
    assert_eq!(partial.consumer_evidence.len(), 1);
    assert!(!partial.consumer_evidence_verified);
    fs::write(
        f.destination.join("inputs/picture.bin"),
        b"corrupt destination",
    )
    .unwrap();
    let failed = plan.run(false, None).unwrap();
    assert_eq!(failed.failure.unwrap().phase, "preflight-destination");
    assert_eq!(failed.consumer_evidence.len(), 1);
    assert!(!failed.consumer_evidence_verified);
    assert_eq!(failed.copied_bytes, 0);
}

#[test]
fn cli_handoff_is_bound_to_frozen_source_and_reports_optional_links() {
    let mut f = Fixture::new();
    let frozen_commit = f.profile["commit"].clone();
    let pin = Object::from_bytes(b"consumer environment metadata");
    f.profile["consumer_evidence"] = json!({"environment":pin});
    fs::write(f.repo.join("unrelated.txt"), b"newer checkout").unwrap();
    git(&f.repo, &["add", "unrelated.txt"]);
    git(&f.repo, &["commit", "-m", "unrelated later source"]);
    let out = f.run(&[]);
    assert!(out.status.success());
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["source_commit"], frozen_commit);
    assert_eq!(report["consumer_evidence"]["environment"], json!(pin));
    assert_eq!(report["consumer_evidence_verified"], false);
    assert_eq!(report["apply"], false);
    assert!(!f.destination.exists());
    // Strict parsing must not silently collapse duplicate consumer evidence labels.
    let raw = String::from_utf8(f.raw()).unwrap();
    let encoded = serde_json::to_string(&f.profile["consumer_evidence"]["environment"]).unwrap();
    let duplicate = raw.replace(
        &format!("\"environment\":{encoded}"),
        &format!("\"environment\":{encoded},\"environment\":{encoded}"),
    );
    assert_ne!(raw, duplicate);
    assert!(project::prepare(duplicate.as_bytes(), "profiles/nested/render.json").is_err());
}
struct Fixture {
    temp: tempfile::TempDir,
    repo: PathBuf,
    store: PathBuf,
    destination: PathBuf,
    profile: Value,
}
fn write(path: &Path, value: &Value) -> Object {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let raw = serde_json::to_vec(value).unwrap();
    fs::write(path, &raw).unwrap();
    Object::from_bytes(&raw)
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let store = temp.path().join("store");
        fs::create_dir(&repo).unwrap();
        fs::create_dir_all(store.join("objects")).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        git(&repo, &["config", "core.autocrlf", "false"]);
        for raw in [ALPHA, BETA] {
            let object = Object::from_bytes(raw);
            fs::write(store.join("objects").join(object.sha256.as_str()), raw).unwrap();
        }
        let schema = write(
            &repo.join("types/shared/authored.json"),
            &json!({"type":"object"}),
        );
        let definition = |name: &str| {
            json!({"schema":"tig.definition.design-1","id":name,"authored_schema":schema,
            "authoring":{"properties":["restore_path"],"required":["restore_path"],"defaults":{}},"stream_rules":{},"semantic_rules":{}})
        };
        let first = write(
            &repo.join("types/shared/file.json"),
            &definition("types/file"),
        );
        let second = write(
            &repo.join("types/scene/file.json"),
            &definition("types/scene"),
        );
        let record = |id: &str,
                      definition: Object,
                      definition_id: &str,
                      object: Object,
                      path: &str,
                      streams: Value| {
            let mut pin = serde_json::to_value(definition).unwrap();
            pin["id"] = json!(definition_id);
            json!({"schema":"tig.instance.design-1","id":id,"definition":pin,"authored":{"restore_path":path},
                "selected":object,"streams":streams,"status":{},"metadata":{}})
        };
        write(
            &repo.join("catalog/view/picture.asset"),
            &record(
                "picture",
                first.clone(),
                "types/file",
                Object::from_bytes(ALPHA),
                "inputs/picture.bin",
                json!([]),
            ),
        );
        write(
            &repo.join("catalog/render/cue.asset"),
            &record(
                "cue",
                second,
                "types/scene",
                Object::from_bytes(BETA),
                "inputs/cue.bin",
                json!([{"slot":"selected","role":"alternate","group":"literal","object":Object::from_bytes(ALPHA)}]),
            ),
        );
        write(
            &repo.join("catalog/host/render-only.asset"),
            &record(
                "render-only",
                first,
                "types/file",
                Object::from_bytes(b"absent render-only bytes"),
                "inputs/host.bin",
                json!([]),
            ),
        );
        let view = json!({"schema":"tig.project-restore.experimental-2","records":["catalog/view"],"definitions":["types/shared"],
            "destination_prefixes":["inputs"],"selection":{"kind":"entries","entries":[{"id":"picture","selector":{"kind":"selected"},"destination":"inputs/view.bin"}]}});
        write(&repo.join("profiles/nested/view.json"), &view);
        let mut render = view.clone();
        render["records"] = json!(["catalog/view", "catalog/render"]);
        render["definitions"] = json!(["types/shared", "types/scene"]);
        render["selection"]["entries"] = json!([
            {"id":"picture","selector":{"kind":"selected"},"destination":"inputs/picture.bin"},
            {"id":"cue","selector":{"kind":"selected"},"destination":"inputs/cue.bin"},
            {"id":"cue","selector":{"kind":"stream","slot":"selected"},"destination":"inputs/alias.bin"}]);
        write(&repo.join("profiles/nested/render.json"), &render);
        git(&repo, &["add", "catalog", "types", "profiles"]);
        git(&repo, &["commit", "-m", "fixture"]);
        let commit = String::from_utf8(git(&repo, &["rev-parse", "HEAD"]).stdout).unwrap();
        let destination = temp.path().join("fresh-owned/run");
        let profile = json!({"schema":"tig.project-restore-profile.experimental-1","git_executable":git_executable(),"repository":repo,
            "commit":commit.trim(),"destination_root":destination,"limits":{"entries":1024,"destination_bytes":1048576},
            "byte_source":{"places":[{"root":store,"layout":"flat"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"}}});
        Self {
            temp,
            repo,
            store,
            destination,
            profile,
        }
    }
    fn raw(&self) -> Vec<u8> {
        serde_json::to_vec(&self.profile).unwrap()
    }
    fn plan(&self, manifest: &str) -> project::Plan {
        project::prepare(&self.raw(), manifest).unwrap()
    }
    fn commit(&mut self) {
        git(&self.repo, &["add", "catalog", "types", "profiles"]);
        git(&self.repo, &["commit", "-m", "update"]);
        self.profile["commit"] = json!(
            String::from_utf8(git(&self.repo, &["rev-parse", "HEAD"]).stdout)
                .unwrap()
                .trim()
        );
    }
    fn manifest(&self) -> Value {
        serde_json::from_slice(&fs::read(self.repo.join("profiles/nested/render.json")).unwrap())
            .unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        let path = self.temp.path().join("local-profile.json");
        fs::write(&path, self.raw()).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_git-asset-project"));
        c.args(args).arg(path).arg("profiles/nested/render.json");
        c.output().unwrap()
    }
}

#[test]
fn nested_preview_and_fresh_root_apply_reuse_bytes_and_preserve_literal_selector() {
    let f = Fixture::new();
    let plan = f.plan("profiles/nested/render.json");
    let preview = plan.run(false, None).unwrap();
    assert!(preview.failure.is_none());
    assert!(!f.destination.exists());
    assert_eq!(preview.unique_records, 2);
    assert_eq!(preview.unique_objects, 2);
    assert_eq!(preview.entries.len(), 3);
    assert_eq!(
        preview.unique_object_bytes,
        (ALPHA.len() + BETA.len()) as u64
    );
    assert_eq!(
        preview.destination_bytes,
        (2 * ALPHA.len() + BETA.len()) as u64
    );
    assert_eq!(preview.source_verifications, 2);
    let result = plan.run(true, Some(plan.identity())).unwrap();
    assert!(
        result.failure.is_none(),
        "{}",
        serde_json::to_string(&result).unwrap()
    );
    assert_eq!(result.copied_bytes, result.destination_bytes);
    assert_eq!(
        fs::read(f.destination.join("inputs/alias.bin")).unwrap(),
        ALPHA
    );
    assert_eq!(
        fs::read(f.destination.join("inputs/cue.bin")).unwrap(),
        BETA
    );
    assert!(
        result
            .entries
            .iter()
            .all(|e| e.destination_verified && e.source_protection_observed_at_copy.is_some())
    );
    assert!(
        !result.source_guards_held
            && !result.independent_custody_verified
            && !result.authorizes_eviction
    );
    let retry = plan.run(true, Some(plan.identity())).unwrap();
    assert!(retry.failure.is_none());
    assert_eq!(retry.copied_bytes, 0);
    assert!(retry.entries.iter().all(|e| e.action == "already-present"));
}

#[test]
fn frozen_metadata_executes_with_git_checkout_unavailable() {
    let f = Fixture::new();
    let plan = f.plan("profiles/nested/render.json");
    fs::rename(&f.repo, f.temp.path().join("repo-offline")).unwrap();
    let result = plan.run(true, Some(plan.identity())).unwrap();
    assert!(result.failure.is_none());
    assert_eq!(
        fs::read(f.destination.join("inputs/picture.bin")).unwrap(),
        ALPHA
    );
}

#[test]
fn view_profile_does_not_require_render_only_inputs() {
    let mut f = Fixture::new();
    assert!(
        f.plan("profiles/nested/view.json")
            .run(false, None)
            .unwrap()
            .failure
            .is_none()
    );
    let mut render = f.manifest();
    render["records"]
        .as_array_mut()
        .unwrap()
        .push(json!("catalog/host"));
    render["selection"]["entries"].as_array_mut().unwrap().push(
        json!({"id":"render-only","selector":{"kind":"selected"},"destination":"inputs/host.bin"}),
    );
    write(&f.repo.join("profiles/nested/render.json"), &render);
    f.commit();
    let failed = f
        .plan("profiles/nested/render.json")
        .run(true, None)
        .unwrap();
    assert_eq!(failed.failure.unwrap().phase, "preflight-source");
    assert!(!f.destination.exists());
}

#[test]
fn case_alias_and_parent_collisions_refuse_before_materialization() {
    for destination in ["inputs/PICTURE.bin", "inputs/picture.bin/child"] {
        let mut f = Fixture::new();
        let mut m = f.manifest();
        m["selection"]["entries"][1]["destination"] = json!(destination);
        write(&f.repo.join("profiles/nested/render.json"), &m);
        f.commit();
        assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
        assert!(!f.destination.exists());
    }
}

#[test]
fn destination_and_reader_budgets_refuse_without_directories() {
    let mut f = Fixture::new();
    f.profile["limits"]["destination_bytes"] = json!(1);
    assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
    f.profile["limits"]["destination_bytes"] = json!(1048576);
    f.profile["byte_source"]["limits"] = json!({"requests":2});
    let p = f.plan("profiles/nested/render.json");
    assert!(p.run(true, None).is_err());
    assert!(!f.destination.exists());
    assert!(p.run(false, None).unwrap().failure.is_none());
}

#[test]
fn changed_source_profile_or_manifest_is_fenced_but_dirty_files_are_ignored() {
    let mut f = Fixture::new();
    let original = f.plan("profiles/nested/render.json");
    let expected = original.identity().clone();
    fs::write(
        f.repo.join("catalog/view/picture.asset"),
        b"dirty invalid metadata",
    )
    .unwrap();
    assert_eq!(f.plan("profiles/nested/render.json").identity(), &expected);
    f.profile["destination_root"] = json!(f.temp.path().join("another-run"));
    assert!(
        f.plan("profiles/nested/render.json")
            .run(true, Some(&expected))
            .is_err()
    );
    f.profile["destination_root"] = json!(f.destination);
    git(&f.repo, &["checkout", "--", "catalog/view/picture.asset"]);
    let mut m = f.manifest();
    m["selection"]["entries"].as_array_mut().unwrap().pop();
    write(&f.repo.join("profiles/nested/render.json"), &m);
    f.commit();
    assert!(
        f.plan("profiles/nested/render.json")
            .run(true, Some(&expected))
            .is_err()
    );
    assert!(!f.destination.exists());
}

#[test]
fn missing_definition_scope_and_duplicate_manifest_keys_refuse() {
    let mut f = Fixture::new();
    let mut m = f.manifest();
    m["definitions"] = json!(["types/shared"]);
    write(&f.repo.join("profiles/nested/render.json"), &m);
    f.commit();
    assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
    fs::write(
        f.repo.join("profiles/nested/render.json"),
        b"{\"schema\":\"x\",\"schema\":\"y\"}",
    )
    .unwrap();
    f.commit();
    assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
    assert!(!f.destination.exists());
}

#[test]
fn corruption_and_existing_destination_conflict_never_partially_start_batch() {
    let f = Fixture::new();
    fs::write(
        f.store
            .join("objects")
            .join(Object::from_bytes(BETA).sha256.as_str()),
        b"corrupt",
    )
    .unwrap();
    let report = f
        .plan("profiles/nested/render.json")
        .run(true, None)
        .unwrap();
    assert!(report.failure.is_some());
    assert!(!f.destination.exists());
    let f = Fixture::new();
    fs::create_dir_all(f.destination.join("inputs")).unwrap();
    fs::write(f.destination.join("inputs/cue.bin"), b"different").unwrap();
    let report = f
        .plan("profiles/nested/render.json")
        .run(true, None)
        .unwrap();
    assert_eq!(report.failure.unwrap().phase, "preflight-destination");
    assert!(!f.destination.join("inputs/picture.bin").exists());
    assert_eq!(
        fs::read(f.destination.join("inputs/cue.bin")).unwrap(),
        b"different"
    );
}

#[test]
fn explicit_progress_stop_preserves_completed_and_pending_then_retry_verifies() {
    let f = Fixture::new();
    let p = f.plan("profiles/nested/render.json");
    let result = p.run_observed(true, Some(p.identity()), |_| false).unwrap();
    assert_eq!(result.failure.unwrap().phase, "interrupted");
    assert_eq!(result.entries[0].action, "restored");
    assert_eq!(result.entries[1].action, "pending");
    assert!(f.destination.join("inputs/picture.bin").exists());
    assert!(!f.destination.join("inputs/cue.bin").exists());
    let retry = p.run(true, Some(p.identity())).unwrap();
    assert!(retry.failure.is_none());
    assert_eq!(retry.entries[0].action, "already-present");
    assert_eq!(retry.copied_bytes, (ALPHA.len() + BETA.len()) as u64);
}

#[test]
fn project_interruption_child() {
    let Some(path) = std::env::var_os("GIT_ASSET_PROJECT_TEST_PROFILE") else {
        return;
    };
    let raw = fs::read(path).unwrap();
    let plan = project::prepare(&raw, "profiles/nested/render.json").unwrap();
    let _ = plan.run_observed(true, Some(plan.identity()), |_| std::process::exit(29));
    panic!("interruption callback did not execute");
}

#[test]
fn actual_child_exit_between_entries_retains_files_and_explicit_retry() {
    let f = Fixture::new();
    let path = f.temp.path().join("child-profile.json");
    fs::write(&path, f.raw()).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "project_interruption_child", "--nocapture"])
        .env("GIT_ASSET_PROJECT_TEST_PROFILE", &path)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(29));
    assert!(f.destination.join("inputs/picture.bin").exists());
    assert!(!f.destination.join("inputs/cue.bin").exists());
    let plan = f.plan("profiles/nested/render.json");
    let report = plan.run(true, Some(plan.identity())).unwrap();
    assert!(report.failure.is_none());
    assert_eq!(report.entries[0].action, "already-present");
}

#[test]
fn compact_catalog_restores_thousand_declared_non_media_assets_without_manifest_expansion() {
    let mut f = Fixture::new();
    let original: Value =
        serde_json::from_slice(&fs::read(f.repo.join("catalog/view/picture.asset")).unwrap())
            .unwrap();
    for n in 0..1000 {
        let mut r = original.clone();
        r["id"] = json!(format!("invoice/{n:04}"));
        r["authored"]["restore_path"] = json!(format!("inputs/invoice-{n:04}.pdf"));
        write(&f.repo.join(format!("invoices/{n:04}.asset")), &r);
    }
    let m = json!({"schema":"tig.project-restore.experimental-2","records":["invoices"],"definitions":["types/shared"],"destination_prefixes":["inputs"],
        "selection":{"kind":"catalog","selector":{"kind":"selected"},"destination_field":"restore_path"}});
    write(&f.repo.join("profiles/nested/invoices.json"), &m);
    git(&f.repo, &["add", "invoices"]);
    f.commit();
    assert!(
        fs::metadata(f.repo.join("profiles/nested/invoices.json"))
            .unwrap()
            .len()
            < 1024
    );
    let r = f
        .plan("profiles/nested/invoices.json")
        .run(false, None)
        .unwrap();
    assert!(r.failure.is_none());
    assert_eq!(r.entries.len(), 1000);
    assert_eq!(r.unique_objects, 1);
    assert_eq!(r.source_verifications, 1);
    assert_eq!(r.destination_bytes, (ALPHA.len() * 1000) as u64);
    assert!(!f.destination.exists());
}

#[test]
fn cli_reports_partial_scope_and_rejects_stale_plan_or_duplicate_flags() {
    let f = Fixture::new();
    let output = f.run(&[]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["apply"], false);
    assert!(!f.destination.exists());
    let plan = report["plan"].as_str().unwrap();
    let output = f.run(&["--apply", "--expect-plan", plan]);
    assert!(output.status.success());
    let wrong = Digest::blob(b"other plan");
    let output = f.run(&["--apply", "--expect-plan", wrong.as_str()]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("code=plan-changed;")
    );
    let output = f.run(&["--apply", "--apply"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap().trim(),
        "project restore refused; verify explicit source, profile and plan\ncode=arguments-invalid; Check flag combinations; put flags before PROFILE MANIFEST."
    );
}

#[test]
fn logged_apply_and_retry_use_fresh_evidence_and_inspection_does_not_reverify() {
    let f = Fixture::new();
    let p = f.plan("profiles/nested/render.json");
    let log = f.temp.path().join("attempt.jsonl");
    let report = p.run_logged(p.identity(), &log).unwrap();
    assert!(report.failure.is_none());
    let original = fs::read(&log).unwrap();
    let summary = p.inspect_progress(p.identity(), &log).unwrap();
    assert_eq!(summary.logged_completed, 3);
    assert_eq!(summary.not_logged_completed, 0);
    assert_eq!(summary.terminal_success, Some(true));
    assert!(!summary.destinations_reverified);
    assert!(!summary.authorizes_eviction);
    assert_eq!(summary.evidence, Object::from_bytes(&original));
    let first_attempt = summary.attempt.clone();
    assert!(p.run_logged(p.identity(), &log).is_err());
    assert_eq!(fs::read(&log).unwrap(), original);
    fs::write(
        f.destination.join("inputs/picture.bin"),
        b"changed afterwards",
    )
    .unwrap();
    // Inspection describes historical evidence, not the bytes currently on disk.
    assert_eq!(
        p.inspect_progress(p.identity(), &log)
            .unwrap()
            .logged_completed,
        3
    );
    let retry_log = f.temp.path().join("retry.jsonl");
    let retry = p.run_logged(p.identity(), &retry_log).unwrap();
    assert_eq!(retry.failure.unwrap().phase, "preflight-destination");
    let retry_summary = p.inspect_progress(p.identity(), &retry_log).unwrap();
    assert_eq!(retry_summary.terminal_success, Some(false));
    assert_ne!(retry_summary.attempt, first_attempt);
}

#[test]
fn logged_observer_stop_and_explicit_retry_recheck_completed_files() {
    let f = Fixture::new();
    let p = f.plan("profiles/nested/render.json");
    let log = f.temp.path().join("stopped.jsonl");
    let r = p
        .run_logged_observed(p.identity(), &log, |_| false)
        .unwrap();
    assert_eq!(r.failure.unwrap().phase, "interrupted");
    let summary = p.inspect_progress(p.identity(), &log).unwrap();
    assert_eq!(summary.logged_completed, 1);
    assert_eq!(summary.not_logged_completed, 2);
    assert_eq!(summary.terminal_success, Some(false));
    let next = f.temp.path().join("next.jsonl");
    let r = p.run_logged(p.identity(), &next).unwrap();
    assert!(r.failure.is_none());
    assert_eq!(r.entries[0].action, "already-present");
    assert_eq!(r.copied_bytes, (ALPHA.len() + BETA.len()) as u64);
    assert_eq!(
        p.inspect_progress(p.identity(), &next)
            .unwrap()
            .terminal_success,
        Some(true)
    );
}

#[test]
fn logged_interruption_child() {
    let Some(path) = std::env::var_os("GIT_ASSET_PROJECT_LOGGED_TEST_PROFILE") else {
        return;
    };
    let raw = fs::read(path).unwrap();
    let plan = project::prepare(&raw, "profiles/nested/render.json").unwrap();
    let log = PathBuf::from(std::env::var_os("GIT_ASSET_PROJECT_LOGGED_TEST_LOG").unwrap());
    let _ = plan.run_logged_observed(plan.identity(), &log, |_| std::process::exit(31));
    panic!("interruption callback did not execute");
}

#[test]
fn killed_child_retains_synced_progress_and_retry_never_trusts_it_as_current() {
    let f = Fixture::new();
    let profile = f.temp.path().join("profile.json");
    fs::write(&profile, f.raw()).unwrap();
    let log = f.temp.path().join("killed.jsonl");
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "logged_interruption_child", "--nocapture"])
        .env("GIT_ASSET_PROJECT_LOGGED_TEST_PROFILE", profile)
        .env("GIT_ASSET_PROJECT_LOGGED_TEST_LOG", &log)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(31));
    let p = f.plan("profiles/nested/render.json");
    let summary = p.inspect_progress(p.identity(), &log).unwrap();
    assert_eq!(summary.logged_completed, 1);
    assert_eq!(summary.not_logged_completed, 2);
    assert_eq!(summary.terminal_success, None);
    assert!(!summary.incomplete_tail);
    assert!(f.destination.join("inputs/picture.bin").exists());
    assert!(!f.destination.join("inputs/cue.bin").exists());
    let retry = p
        .run_logged(p.identity(), &f.temp.path().join("retry.jsonl"))
        .unwrap();
    assert!(retry.failure.is_none());
    assert_eq!(retry.entries[0].action, "already-present");
}

#[test]
fn progress_torn_tail_and_missing_completion_remain_unknown_not_failed_copies() {
    let f = Fixture::new();
    let p = f.plan("profiles/nested/render.json");
    let log = f.temp.path().join("progress.jsonl");
    p.run_logged(p.identity(), &log).unwrap();
    let raw = fs::read(&log).unwrap();
    let lines: Vec<_> = raw.split(|b| *b == b'\n').collect();
    // Started + one completion + a torn second completion. Never acknowledge tail.
    let mut torn = lines[0].to_vec();
    torn.push(b'\n');
    torn.extend_from_slice(lines[1]);
    torn.extend_from_slice(b"\n{\"event\":\"completed\"");
    fs::write(&log, &torn).unwrap();
    let s = p.inspect_progress(p.identity(), &log).unwrap();
    assert_eq!(s.logged_completed, 1);
    assert_eq!(s.not_logged_completed, 2);
    assert!(s.incomplete_tail);
    assert_eq!(s.terminal_success, None);
    // A copy can be present without its completion being logged.
    fs::write(&log, [lines[0], b"\n"].concat()).unwrap();
    assert_eq!(
        p.inspect_progress(p.identity(), &log)
            .unwrap()
            .logged_completed,
        0
    );
    let retry = p.run(true, Some(p.identity())).unwrap();
    assert!(retry.failure.is_none());
    assert_eq!(retry.copied_bytes, 0);
    assert!(retry.entries.iter().all(|e| e.action == "already-present"));
}

#[test]
fn progress_rejects_stale_identity_reordered_rows_unknown_fields_and_bad_bounds() {
    let mut f = Fixture::new();
    let p = f.plan("profiles/nested/render.json");
    let log = f.temp.path().join("progress.jsonl");
    p.run_logged(p.identity(), &log).unwrap();
    let raw = fs::read(&log).unwrap();
    let rows: Vec<Value> = raw
        .split(|b| *b == b'\n')
        .filter(|r| !r.is_empty())
        .map(|r| serde_json::from_slice(r).unwrap())
        .collect();
    let encode = |rows: &[Value]| {
        rows.iter()
            .flat_map(|row| {
                let mut r = serde_json::to_vec(row).unwrap();
                r.push(b'\n');
                r
            })
            .collect::<Vec<_>>()
    };
    let mut invalid = rows.clone();
    invalid.swap(1, 2);
    fs::write(&log, encode(&invalid)).unwrap();
    assert!(p.inspect_progress(p.identity(), &log).is_err());
    let mut invalid = rows.clone();
    invalid[1]["unexpected"] = json!(true);
    fs::write(&log, encode(&invalid)).unwrap();
    assert!(p.inspect_progress(p.identity(), &log).is_err());
    let mut invalid = rows.clone();
    invalid.push(rows[1].clone());
    fs::write(&log, encode(&invalid)).unwrap();
    assert!(p.inspect_progress(p.identity(), &log).is_err());
    // Duplicate JSON keys refuse, even when both values agree.
    let first = String::from_utf8(serde_json::to_vec(&rows[0]).unwrap()).unwrap();
    fs::write(&log, format!("{{\"event\":\"started\",{}\n", &first[1..])).unwrap();
    assert!(p.inspect_progress(p.identity(), &log).is_err());
    fs::write(&log, vec![b'x'; 4 * 1024 * 1024 + 1]).unwrap();
    assert!(p.inspect_progress(p.identity(), &log).is_err());
    fs::write(&log, &raw).unwrap();
    f.profile["limits"]["destination_bytes"] = json!(100000);
    let changed = f.plan("profiles/nested/render.json");
    assert!(changed.inspect_progress(changed.identity(), &log).is_err());
    let fresh = f.temp.path().join("must-not-exist.jsonl");
    assert!(changed.run_logged(p.identity(), &fresh).is_err());
    assert!(!fresh.exists());
}

#[test]
fn progress_paths_and_cli_require_explicit_apply_plan_and_fresh_external_file() {
    let f = Fixture::new();
    let p = f.plan("profiles/nested/render.json");
    assert!(
        p.run_logged(p.identity(), Path::new("relative.jsonl"))
            .is_err()
    );
    assert!(
        p.run_logged(p.identity(), &f.destination.join("log.jsonl"))
            .is_err()
    );
    assert!(
        p.run_logged(p.identity(), &f.repo.join("log.jsonl"))
            .is_err()
    );
    assert!(
        p.run_logged(p.identity(), &f.store.join("log.jsonl"))
            .is_err()
    );
    assert!(
        p.run_logged(
            p.identity(),
            &f.temp.path().join("missing-parent/log.jsonl")
        )
        .is_err()
    );
    assert!(!f.destination.exists());
    let path = f.temp.path().join("cli-progress.jsonl");
    let path = path.to_str().unwrap();
    for args in [
        vec!["--progress", path],
        vec!["--apply", "--progress", path],
        vec!["--inspect-progress", path],
        vec![
            "--apply",
            "--inspect-progress",
            path,
            "--expect-plan",
            p.identity().as_str(),
        ],
    ] {
        assert!(!f.run(&args).status.success());
        assert!(!Path::new(path).exists());
    }
    assert!(
        f.run(&[
            "--apply",
            "--expect-plan",
            p.identity().as_str(),
            "--progress",
            path
        ])
        .status
        .success()
    );
    let output = f.run(&[
        "--inspect-progress",
        path,
        "--expect-plan",
        p.identity().as_str(),
    ]);
    assert!(output.status.success());
    let s: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(s["logged_completed"], 3);
    assert_eq!(s["destinations_reverified"], false);
}

fn require(f: &mut Fixture, requirements: Value) {
    let mut manifest = f.manifest();
    manifest["requirements"] = requirements;
    write(&f.repo.join("profiles/nested/render.json"), &manifest);
    f.commit();
}

#[test]
fn declared_hosts_report_unbound_missing_and_wrong_before_source_reads() {
    let mut f = Fixture::new();
    let wrong = f.temp.path().join("wrong-font.bin");
    fs::write(&wrong, b"not the font").unwrap();
    let missing = f.temp.path().join("absent-tool.bin");
    f.profile["host_files"] = json!({"font/wrong":wrong,"tool/missing":missing});
    require(
        &mut f,
        json!({"host_files":[
        {"id":"font/unbound","object":Object::from_bytes(ALPHA)},
        {"id":"font/wrong","object":Object::from_bytes(ALPHA)},
        {"id":"tool/missing","object":Object::from_bytes(BETA)}]}),
    );
    fs::write(
        f.store
            .join("objects")
            .join(Object::from_bytes(ALPHA).sha256.as_str()),
        b"also corrupt",
    )
    .unwrap();
    let r = f
        .plan("profiles/nested/render.json")
        .run(true, None)
        .unwrap();
    assert_eq!(r.failure.unwrap().phase, "preflight-host");
    assert_eq!(
        r.host_files.iter().map(|h| h.outcome).collect::<Vec<_>>(),
        ["unbound", "different-or-unreadable", "missing"]
    );
    assert_eq!(r.source_verifications, 0);
    assert_eq!(r.copied_bytes, 0);
    assert!(!f.destination.exists());
    let raw = serde_json::to_string(&r.host_files).unwrap();
    assert!(!raw.contains("wrong-font.bin"));
    assert!(r.host_files.iter().all(|h| !h.protection_held));
}

#[test]
fn view_ignores_render_host_and_capacity_bindings_without_discovery() {
    let mut f = Fixture::new();
    f.profile["host_files"] = json!({"font/arial":f.temp.path().join("does-not-exist")});
    f.profile["capacity_root"] = json!(f.temp.path().join("unavailable-drive"));
    require(
        &mut f,
        json!({"host_files":[{"id":"font/arial","object":Object::from_bytes(ALPHA)}]}),
    );
    let view = f
        .plan("profiles/nested/view.json")
        .run(false, None)
        .unwrap();
    assert!(view.failure.is_none());
    assert!(view.host_files.is_empty());
    assert!(view.capacity.is_none());
    assert_eq!(view.source_verifications, 1);
    assert!(!f.destination.exists());
    let render = f
        .plan("profiles/nested/render.json")
        .run(false, None)
        .unwrap();
    assert_eq!(render.failure.unwrap().phase, "preflight-host");
}

#[test]
fn host_relocation_preserves_object_identity_and_apply_rechecks_after_preview() {
    let mut f = Fixture::new();
    let a = f.temp.path().join("host-a");
    let b = f.temp.path().join("host-b");
    fs::write(&a, ALPHA).unwrap();
    fs::write(&b, ALPHA).unwrap();
    f.profile["host_files"] = json!({"font/arial":a});
    require(
        &mut f,
        json!({"host_files":[{"id":"font/arial","object":Object::from_bytes(ALPHA)}]}),
    );
    let first = f.plan("profiles/nested/render.json");
    let preview = first.run(false, None).unwrap();
    assert!(preview.failure.is_none());
    assert!(preview.host_files[0].verified_at_preflight);
    f.profile["host_files"]["font/arial"] = json!(b);
    let second = f.plan("profiles/nested/render.json");
    assert_ne!(first.identity(), second.identity());
    assert!(second.run(true, Some(first.identity())).is_err());
    let preview = second.run(false, None).unwrap();
    assert_eq!(preview.host_files[0].object, Object::from_bytes(ALPHA));
    // Same length but wrong bytes must refuse, despite an earlier valid preview.
    fs::write(&b, vec![b'x'; ALPHA.len()]).unwrap();
    let result = second.run(true, Some(second.identity())).unwrap();
    assert_eq!(result.failure.unwrap().phase, "preflight-host");
    assert!(!f.destination.exists());
}

#[test]
fn host_limits_duplicates_unknown_fields_and_estimate_overflow_refuse_planning() {
    for requirements in [
        json!({"host_files":[{"id":"same","object":Object::from_bytes(ALPHA)},{"id":"same","object":Object::from_bytes(ALPHA)}]}),
        json!({"host_files":[{"id":"large","object":{"sha256":Object::from_bytes(ALPHA).sha256,"bytes":256_u64*1024*1024+1}}]}),
        json!({"host_files":(0..17).map(|n| json!({"id":format!("host/{n}"),"object":Object::from_bytes(ALPHA)})).collect::<Vec<_>>()}),
        json!({"storage":{"minimum_available_bytes":0,"scratch_estimate_bytes":u64::MAX}}),
        json!({"execute":"not-allowed"}),
    ] {
        let mut f = Fixture::new();
        require(&mut f, requirements);
        assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
        assert!(!f.destination.exists());
    }
    let mut f = Fixture::new();
    f.profile["limits"]["host_verification_bytes"] = json!(0);
    require(
        &mut f,
        json!({"host_files":[{"id":"file","object":Object::from_bytes(ALPHA)}]}),
    );
    assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
    f.profile["limits"]["host_verification_bytes"] = json!(512_u64 * 1024 * 1024 + 1);
    assert!(project::prepare(&f.raw(), "profiles/nested/view.json").is_err());
    f.profile["limits"]["host_verification_bytes"] = json!(1024);
    f.profile["host_files"] = json!({"file":"relative-font"});
    assert!(project::prepare(&f.raw(), "profiles/nested/render.json").is_err());
}

#[test]
fn capacity_observations_refuse_unbound_offline_unrelated_and_insufficient_roots() {
    let mut f = Fixture::new();
    require(
        &mut f,
        json!({"storage":{"minimum_available_bytes":9_007_199_254_740_991_u64}}),
    );
    let report = f
        .plan("profiles/nested/render.json")
        .run(true, None)
        .unwrap();
    assert_eq!(report.failure.unwrap().phase, "preflight-capacity");
    assert_eq!(report.capacity.unwrap().outcome, "unbound");
    f.profile["capacity_root"] = json!(f.temp.path().join("offline"));
    f.profile["destination_root"] = json!(f.temp.path().join("offline/run"));
    let report = f
        .plan("profiles/nested/render.json")
        .run(true, None)
        .unwrap();
    assert_eq!(report.capacity.unwrap().outcome, "unavailable-root");
    f.profile["destination_root"] = json!(f.destination);
    f.profile["capacity_root"] = json!(f.store);
    let report = f
        .plan("profiles/nested/render.json")
        .run(true, None)
        .unwrap();
    assert_eq!(report.capacity.unwrap().outcome, "unrelated-root");
    f.profile["capacity_root"] = json!(f.temp.path());
    let report = f
        .plan("profiles/nested/render.json")
        .run(true, None)
        .unwrap();
    let capacity = report.capacity.unwrap();
    assert_eq!(report.failure.unwrap().phase, "preflight-capacity");
    assert_eq!(capacity.outcome, "observed");
    assert_eq!(capacity.sufficient_for_logical_restore, Some(false));
    assert!(!capacity.capacity_admission_granted);
    assert!(!capacity.quota_enforced);
    assert_eq!(report.source_verifications, 0);
    assert!(!f.destination.exists());
    assert!(!f.temp.path().join("offline").exists());
}

#[test]
fn capacity_reports_alias_destination_bytes_then_zero_new_bytes_on_verified_retry() {
    let mut f = Fixture::new();
    f.profile["capacity_root"] = json!(f.temp.path());
    require(
        &mut f,
        json!({"storage":{"minimum_available_bytes":0,"scratch_estimate_bytes":123}}),
    );
    let p = f.plan("profiles/nested/render.json");
    let r = p.run(false, None).unwrap();
    assert!(r.failure.is_none());
    let c = r.capacity.unwrap();
    assert_eq!(
        c.new_logical_destination_bytes,
        (ALPHA.len() * 2 + BETA.len()) as u64
    );
    assert_eq!(
        c.estimated_peak_logical_bytes,
        c.new_logical_destination_bytes + 123
    );
    assert_eq!(c.sufficient_for_logical_restore, Some(true));
    assert!(!c.capacity_admission_granted);
    assert!(!f.destination.exists());
    let r = p.run(true, Some(p.identity())).unwrap();
    assert!(r.failure.is_none());
    let retry = p.run(false, Some(p.identity())).unwrap();
    assert!(retry.failure.is_none());
    assert_eq!(retry.capacity.unwrap().new_logical_destination_bytes, 0);
    assert_eq!(
        retry.destination_bytes,
        (ALPHA.len() * 2 + BETA.len()) as u64
    );
}

#[test]
fn estimate_shortfall_is_reported_separately_from_required_space_threshold() {
    let mut f = Fixture::new();
    f.profile["capacity_root"] = json!(f.temp.path());
    require(
        &mut f,
        json!({"storage":{"minimum_available_bytes":0,"scratch_estimate_bytes":9_007_199_254_740_000_u64}}),
    );
    let r = f
        .plan("profiles/nested/render.json")
        .run(false, None)
        .unwrap();
    assert!(r.failure.is_none());
    let c = r.capacity.unwrap();
    assert_eq!(c.sufficient_for_logical_restore, Some(true));
    assert_eq!(c.sufficient_for_estimated_peak, Some(false));
    assert!(!f.destination.exists());
}

#[test]
fn host_failure_progress_is_terminal_and_fresh_explicit_retry_checks_the_file() {
    let mut f = Fixture::new();
    let font = f.temp.path().join("font");
    f.profile["host_files"] = json!({"font/arial":font});
    require(
        &mut f,
        json!({"host_files":[{"id":"font/arial","object":Object::from_bytes(ALPHA)}]}),
    );
    let p = f.plan("profiles/nested/render.json");
    let log = f.temp.path().join("failed.jsonl");
    assert_eq!(
        p.run_logged(p.identity(), &log)
            .unwrap()
            .failure
            .unwrap()
            .phase,
        "preflight-host"
    );
    let summary = p.inspect_progress(p.identity(), &log).unwrap();
    assert_eq!(summary.terminal_success, Some(false));
    assert_eq!(summary.logged_completed, 0);
    assert!(!f.destination.exists());
    fs::write(font, ALPHA).unwrap();
    let retry = p
        .run_logged(p.identity(), &f.temp.path().join("retry.jsonl"))
        .unwrap();
    assert!(retry.failure.is_none());
    assert!(retry.host_files[0].verified_at_preflight);
}
