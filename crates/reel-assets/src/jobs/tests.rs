//! Real Windows execution in synthetic active caches, without production assets.
use super::*;
use crate::tiering::tests::Fixture;
use std::{process::Command, sync::OnceLock};

fn tool() -> &'static [u8] {
    static TOOL: OnceLock<Vec<u8>> = OnceLock::new();
    TOOL.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("fixture.exe");
        let output = Command::new("rustc")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/job_lifecycle.rs"
            ))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::read(executable).unwrap()
    })
}

fn prepare(f: &Fixture, mode: &str) -> (PathBuf, Value) {
    let executable = tool();
    let oid = f.put(executable, true);
    let source = f.put(b"source", true);
    let manifest = f.root().join(format!("{mode}.json"));
    let job = json!({"budget_bytes":16*1024*1024,"scratch_bytes":2*1024*1024,
        "inputs":{"in.bin":{"sha256":source,"bytes":6}},
        "tools":{"tools/tool.exe":{"sha256":oid,"bytes":executable.len()}},
        "steps":[{"tool":"tools/tool.exe","args":[mode,"in.bin","out.bin"],"timeout_seconds":10}],
        "outputs":{"copy":{"path":"out.bin","max_bytes":1024}}});
    fs::write(&manifest, job.to_string()).unwrap();
    (manifest, job)
}

fn evidence(f: &Fixture, oid: &str) -> Value {
    let cfg = config(f.root()).unwrap();
    let path = cfg
        .cache_roots
        .iter()
        .map(|c| object(c, oid))
        .find(|p| p.exists())
        .unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn failed_evidence(f: &Fixture, error: &str) -> Value {
    let (_, rest) = error.split_once("cache://sha256/").expect(error);
    evidence(f, &rest[..64])
}

fn no_scratch(f: &Fixture) {
    for cache in config(f.root()).unwrap().cache_roots {
        let p = cache.join("staging/caminos");
        assert!(!p.exists() || fs::read_dir(p).unwrap().next().is_none());
    }
    assert!(!f.root().join("offline-cold").exists());
}

#[test]
fn default_success_shares_inputs_publishes_evidence_and_retires_scratch() {
    let f = Fixture::new();
    let (manifest, job) = prepare(&f, "copy");
    let result = run(f.root(), &manifest).unwrap();
    assert_eq!(result["workspace_retained"], false);
    assert_eq!(result["copied_input_bytes"], 0);
    assert_eq!(result["linked_input_bytes"], tool().len() as u64 + 6);
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"source"
    );
    let e = evidence(
        &f,
        result["retirement_evidence"]["sha256"].as_str().unwrap(),
    );
    assert_eq!(
        serde_json::from_str::<Value>(e["request_utf8"].as_str().unwrap()).unwrap(),
        job
    );
    assert!(e["diagnostics"]["step-0.log"].is_object());
    let registered = selected(f.root()).unwrap();
    assert!(registered.contains_key(result["retirement_evidence"]["sha256"].as_str().unwrap()));
    assert!(registered.contains_key(result["manifest_sha256"].as_str().unwrap()));
    no_scratch(&f);
}

#[test]
fn failed_job_salvages_declared_candidate_and_logs_but_retires_intermediates() {
    let f = Fixture::new();
    let (manifest, _) = prepare(&f, "fail");
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("exited 23"), "{error}");
    let e = failed_evidence(&f, &error);
    assert!(
        e["diagnostics"]["step-0.log"]["text"]
            .as_str()
            .unwrap()
            .contains("synthetic failure detail")
    );
    let output = &e["declared_retained_outputs"]["copy"];
    let oid = output["sha256"].as_str().unwrap();
    assert_eq!(selected(f.root()).unwrap()[oid], 9);
    assert_eq!(
        fs::read(object(&f.root().join("fast"), oid)).unwrap(),
        b"candidate"
    );
    assert!(!e.to_string().contains("intermediate.bin"));
    no_scratch(&f);
}

#[test]
fn installed_runtime_is_pinned_and_runs_without_an_executable_workspace_copy() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "copy");
    let installed = f.root().join("installed.exe");
    fs::write(&installed, tool()).unwrap();
    let descriptor = job["tools"]["tools/tool.exe"].clone();
    job["tools"] = json!({});
    job["installed_tools"] = json!({"runtime":{"path":installed,"sha256":descriptor["sha256"],"bytes":descriptor["bytes"]}});
    job["steps"][0]["tool"] = json!("runtime");
    fs::write(&manifest, job.to_string()).unwrap();
    let result = run(f.root(), &manifest).unwrap();
    assert_eq!(result["linked_input_bytes"], 6);
    assert_eq!(result["copied_input_bytes"], 0);
    no_scratch(&f);
    fs::write(&installed, b"changed installation").unwrap();
    assert!(run(f.root(), &manifest).is_err());
    no_scratch(&f);
}

#[test]
fn immutable_input_rejects_mutation_and_explicit_writable_copy_preserves_canonical() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "mutate");
    let original = object(
        &f.root().join("fast"),
        job["inputs"]["in.bin"]["sha256"].as_str().unwrap(),
    );
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("staging retired"), "{error}");
    assert_eq!(fs::read(&original).unwrap(), b"source");
    no_scratch(&f);
    job["writable_inputs"] = json!(["in.bin"]);
    fs::write(&manifest, job.to_string()).unwrap();
    let result = run(f.root(), &manifest).unwrap();
    assert_eq!(result["copied_input_bytes"], 6);
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"changed"
    );
    assert_eq!(fs::read(original).unwrap(), b"source");
    no_scratch(&f);
}

#[test]
fn independent_readers_coexist_and_temporary_links_are_removable_while_pinned() {
    let f = Fixture::new();
    let original = object(&f.root().join("fast"), &f.put(b"immutable", true));
    let link = f.root().join("view");
    let a = lifecycle::pin_read_only(&original).unwrap();
    let b = lifecycle::pin_read_only(&original).unwrap();
    fs::hard_link(&original, &link).unwrap();
    let c = lifecycle::pin_read_only(&link).unwrap();
    assert!(fs::write(&original, b"bad").is_err());
    assert!(fs::write(&link, b"bad").is_err());
    fs::remove_file(link).unwrap();
    assert_eq!(fs::read(&original).unwrap(), b"immutable");
    drop((a, b, c));
}

#[test]
fn concurrent_jobs_share_canonical_inputs_without_a_job_wide_cache_lock() {
    let f = Fixture::new();
    let (manifest, _) = prepare(&f, "hold-copy");
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|s| {
        let execute = || {
            barrier.wait();
            run(f.root(), &manifest).unwrap()
        };
        let a = s.spawn(execute);
        let b = s.spawn(execute);
        for result in [a.join().unwrap(), b.join().unwrap()] {
            assert_eq!(result["workspace_retained"], false);
            assert_eq!(result["copied_input_bytes"], 0);
        }
    });
    no_scratch(&f);
}

#[test]
fn unsafe_salvage_and_registration_failure_hold_the_workspace() {
    let f = Fixture::new();
    let (manifest, _) = prepare(&f, "oversize");
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("staging preserved"), "{error}");
    assert!(error.contains("salvage bound"), "{error}");
    let f = Fixture::new();
    let (manifest, job) = prepare(&f, "copy");
    let oid = job["inputs"]["in.bin"]["sha256"].as_str().unwrap();
    let pointer = f
        .root()
        .join("assets/objects/sha256")
        .join(&oid[..2])
        .join(oid);
    fs::create_dir_all(pointer.parent().unwrap()).unwrap();
    fs::write(pointer, b"corrupt pointer").unwrap();
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("staging preserved"), "{error}");
    let parent = f.root().join("fast/staging/caminos");
    let workspace = fs::read_dir(parent)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(workspace.join("out.bin").exists());
}

#[test]
fn explicit_debug_retention_still_supports_guarded_discard() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "copy");
    job["retain_workspace"] = json!(true);
    let bytes = serde_json::to_vec(&job).unwrap();
    fs::write(&manifest, &bytes).unwrap();
    let oid = f.put(&bytes, true);
    let result = run(f.root(), &manifest).unwrap();
    assert_eq!(result["workspace_retained"], true);
    let receipt = discard(f.root(), result["job_id"].as_str().unwrap(), &oid, false).unwrap();
    assert_eq!(receipt["cache_objects_removed"], 0);
    no_scratch(&f);
}

#[test]
fn removal_of_canonical_names_does_not_delete_the_last_remaining_input_view() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "unlink-originals");
    let oid = job["inputs"]["in.bin"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    // Simulate external name removal while the job holds its input handles.
    job["steps"][0]["args"]
        .as_array_mut()
        .unwrap()
        .extend(["fast", "history"].map(|tier| json!(object(&f.root().join(tier), &oid))));
    // Produce different output so publication does not incidentally restore the original.
    job["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({"tool":"tools/tool.exe",
        "args":["fail","in.bin","out.bin"],"timeout_seconds":10}));
    fs::write(&manifest, job.to_string()).unwrap();
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("Retained original absent"), "{error}");
    assert!(error.contains("staging preserved"), "{error}");
    let workspace = fs::read_dir(f.root().join("fast/staging/caminos"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(workspace.join("in.bin")).unwrap(), b"source");
}

#[test]
fn staging_parent_junction_cannot_redirect_job_writes_or_retirement_outside_cache() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    let destination = outside.path().canonicalize().unwrap();
    fs::write(destination.join("preserve.txt"), b"outside").unwrap();
    let staging = f.root().join("fast/staging");
    let cmd_path = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_start_matches("\\\\?\\")
            .to_owned()
    };
    let output = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(cmd_path(&staging))
        .arg(cmd_path(&destination))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (manifest, _) = prepare(&f, "copy");
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(
        error.contains("escaped its exact admitted cache"),
        "{error}"
    );
    assert!(!destination.join("caminos").exists());
    assert_eq!(
        fs::read(destination.join("preserve.txt")).unwrap(),
        b"outside"
    );
    // Remove only this exact test junction, not its target tree.
    assert!(staging.starts_with(f.root()));
    fs::remove_dir(staging).unwrap();
}

#[test]
fn timeout_retires_workspace_after_descendants_die_and_preserves_pid_evidence() {
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "tree");
    job["steps"][0]["timeout_seconds"] = json!(2);
    fs::write(&manifest, job.to_string()).unwrap();
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("timed out"), "{error}");
    let e = failed_evidence(&f, &error);
    for name in ["parent.pid", "child.pid"] {
        let pid = e["diagnostics"][name]["text"]
            .as_str()
            .unwrap()
            .parse::<u32>()
            .unwrap();
        // A missing PID handle is also evidence of exit; otherwise await termination.
        unsafe {
            let handle = OpenProcess(0x0010_0000 /* SYNCHRONIZE */, 0, pid);
            if !handle.is_null() {
                let status = WaitForSingleObject(handle, 5000);
                CloseHandle(handle);
                assert_eq!(status, WAIT_OBJECT_0);
            }
        }
    }
    no_scratch(&f);
}

fn checkpoint_fixture(f: &Fixture) -> (PathBuf, PathBuf, Value) {
    let (manifest, mut job) = prepare(f, "copy");
    // The skipped step would fail. Resumption must use the exact original
    // recipe and the registered checkpoint, rather than re-executing it.
    job["steps"] = json!([
        {"tool":"tools/tool.exe","args":["fail","in.bin","out.bin"],"timeout_seconds":10},
        {"tool":"tools/tool.exe","args":["mutate","prepared/checkpoint.bin","out.bin"],"timeout_seconds":10}
    ]);
    let bytes = serde_json::to_vec(&job).unwrap();
    fs::write(&manifest, &bytes).unwrap();
    let job_hash = f.put(&bytes, true);
    let checkpoint_hash = f.put(b"checkpoint", true);
    let seed = json!({"schema":"reel.native-job-resume.v1","job_sha256":job_hash,
        "source_job_id":"job-original","start_step":1,
        "files":{"prepared/checkpoint.bin":{"sha256":checkpoint_hash,"bytes":10}}});
    let file = f.root().join("checkpoint.json");
    let bytes = serde_json::to_vec(&seed).unwrap();
    fs::write(&file, &bytes).unwrap();
    f.put(&bytes, true);
    (manifest, file, seed)
}

#[test]
fn checkpoint_resume_keeps_recipe_and_canonical_seed_immutable_and_retires_new_job() {
    let f = Fixture::new();
    let (manifest, seed_path, seed) = checkpoint_fixture(&f);
    let original = fs::read(&manifest).unwrap();
    let result = resume(f.root(), &manifest, &seed_path).unwrap();
    assert_eq!(result["workspace_retained"], false);
    assert_eq!(result["manifest_sha256"], seed["job_sha256"]);
    assert_eq!(result["checkpoint_resume"]["first_executed_step"], 1);
    assert_eq!(fs::read(&manifest).unwrap(), original);
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"changed"
    );
    let oid = seed["files"]["prepared/checkpoint.bin"]["sha256"]
        .as_str()
        .unwrap();
    assert_eq!(
        fs::read(object(&f.root().join("fast"), oid)).unwrap(),
        b"checkpoint"
    );
    let retained = evidence(
        &f,
        result["retirement_evidence"]["sha256"].as_str().unwrap(),
    );
    assert!(retained["diagnostics"].get("step-0.log").is_none());
    assert!(retained["diagnostics"]["step-1.log"].is_object());
    no_scratch(&f);
}

#[test]
fn checkpoint_resume_rejects_changed_recipe_scope_escape_and_corruption() {
    for mode in [
        "changed-recipe",
        "alias",
        "escape",
        "over-budget",
        "wrong-step",
        "corrupt",
    ] {
        let f = Fixture::new();
        let (manifest, seed_path, mut seed) = checkpoint_fixture(&f);
        match mode {
            "changed-recipe" => {
                seed["job_sha256"] = json!("a".repeat(64));
            }
            "alias" => {
                seed["files"] = json!({"in.bin":seed["files"]["prepared/checkpoint.bin"].clone()});
            }
            "escape" => {
                seed["files"] =
                    json!({"../escape.bin":seed["files"]["prepared/checkpoint.bin"].clone()});
            }
            "over-budget" => {
                seed["files"]["prepared/checkpoint.bin"]["bytes"] = json!(3 * 1024 * 1024);
            }
            "wrong-step" => {
                seed["start_step"] = json!(2);
            }
            "corrupt" => {
                let oid = seed["files"]["prepared/checkpoint.bin"]["sha256"]
                    .as_str()
                    .unwrap();
                fs::write(object(&f.root().join("fast"), oid), b"corruption").unwrap();
            }
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&seed).unwrap();
        fs::write(&seed_path, &bytes).unwrap();
        f.put(&bytes, true);
        assert!(resume(f.root(), &manifest, &seed_path).is_err(), "{mode}");
        no_scratch(&f);
    }
}

#[test]
fn registered_recovery_window_extends_only_the_selected_step_and_preserves_recipe() {
    let f = Fixture::new();
    let (manifest, seed_path, mut seed) = checkpoint_fixture(&f);
    let mut job: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    job["steps"][1]["args"] = json!(["hold-copy", "prepared/checkpoint.bin", "out.bin"]);
    job["steps"][1]["timeout_seconds"] = json!(1);
    let original = serde_json::to_vec(&job).unwrap();
    fs::write(&manifest, &original).unwrap();
    seed["job_sha256"] = json!(f.put(&original, true));
    let bytes = serde_json::to_vec(&seed).unwrap();
    fs::write(&seed_path, &bytes).unwrap();
    f.put(&bytes, true);
    let error = resume(f.root(), &manifest, &seed_path)
        .unwrap_err()
        .to_string();
    assert!(error.contains("timed out"), "{error}");
    assert_eq!(
        failed_evidence(&f, &error)["result"]["checkpoint_resume"]["execution_window_explicitly_extended"],
        false
    );

    seed["schema"] = json!("reel.native-job-resume.v2");
    seed["step_timeout_seconds"] = json!({"1": 5});
    let bytes = serde_json::to_vec(&seed).unwrap();
    fs::write(&seed_path, &bytes).unwrap();
    f.put(&bytes, true);
    let result = resume(f.root(), &manifest, &seed_path).unwrap();
    assert_eq!(
        result["checkpoint_resume"]["effective_step_timeout_seconds"]["1"],
        5
    );
    assert_eq!(
        result["checkpoint_resume"]["execution_window_explicitly_extended"],
        true
    );
    assert_eq!(fs::read(&manifest).unwrap(), original);
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"checkpoint"
    );
    no_scratch(&f);
}

#[test]
fn recovery_window_rejects_implicit_unbounded_or_unrelated_step_changes() {
    for (schema, windows) in [
        ("reel.native-job-resume.v1", json!({"1": 11})),
        ("reel.native-job-resume.v2", json!({})),
        ("reel.native-job-resume.v2", json!({"0": 11})),
        ("reel.native-job-resume.v2", json!({"2": 11})),
        ("reel.native-job-resume.v2", json!({"1": 0})),
        ("reel.native-job-resume.v2", json!({"1": 10})),
        ("reel.native-job-resume.v2", json!({"1": 7201})),
        ("reel.native-job-resume.v2", json!({"0": 11, "1": 11})),
    ] {
        let f = Fixture::new();
        let (manifest, seed_path, mut seed) = checkpoint_fixture(&f);
        seed["schema"] = json!(schema);
        seed["step_timeout_seconds"] = windows.clone();
        let bytes = serde_json::to_vec(&seed).unwrap();
        fs::write(&seed_path, &bytes).unwrap();
        f.put(&bytes, true);
        let error = resume(f.root(), &manifest, &seed_path)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("scoped recovery execution window"),
            "{schema} {windows}: {error}"
        );
        no_scratch(&f);
    }
}

#[test]
fn fresh_registered_window_runs_all_steps_and_preserves_original_timeout_controls() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "hold-copy");
    job["steps"][0]["timeout_seconds"] = json!(1);
    let original = serde_json::to_vec(&job).unwrap();
    fs::write(&manifest, &original).unwrap();
    let hash = f.put(&original, true);
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("timed out"), "{error}");
    let policy = json!({"schema":"reel.native-job-execution-window.v1",
        "job_sha256":hash,"step_timeout_seconds":{"0":5}});
    let bytes = serde_json::to_vec(&policy).unwrap();
    let path = f.root().join("execution-window.json");
    fs::write(&path, &bytes).unwrap();
    assert!(run_with_execution_window(f.root(), &manifest, &path).is_err());
    no_scratch(&f);
    f.put(&bytes, true);
    let result = run_with_execution_window(f.root(), &manifest, &path).unwrap();
    assert!(result.get("checkpoint_resume").is_none());
    assert_eq!(result["execution_window"]["steps_skipped"], false);
    assert_eq!(
        result["execution_window"]["effective_step_timeout_seconds"]["0"],
        5
    );
    assert_eq!(result["manifest_sha256"], hash);
    assert_eq!(fs::read(&manifest).unwrap(), original);
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"source"
    );
    no_scratch(&f);
}

#[test]
fn fresh_window_rejects_changed_identity_extra_fields_and_checkpoint_commands() {
    for mode in [
        "schema",
        "recipe",
        "extra-field",
        "empty",
        "too-long",
        "multiple",
        "not-longer",
        "resume-command",
    ] {
        let f = Fixture::new();
        let (manifest, _) = prepare(&f, "copy");
        let original = fs::read(&manifest).unwrap();
        let hash = f.put(&original, true);
        let mut policy = json!({"schema":"reel.native-job-execution-window.v1",
            "job_sha256":hash,"step_timeout_seconds":{"0":11}});
        match mode {
            "schema" => policy["schema"] = json!("unknown"),
            "recipe" => policy["job_sha256"] = json!("a".repeat(64)),
            "extra-field" => policy["files"] = json!({}),
            "empty" => policy["step_timeout_seconds"] = json!({}),
            "too-long" => policy["step_timeout_seconds"] = json!({"0":7201}),
            "multiple" => policy["step_timeout_seconds"] = json!({"0":11,"1":11}),
            "not-longer" => policy["step_timeout_seconds"] = json!({"0":10}),
            "resume-command" => {}
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&policy).unwrap();
        let path = f.root().join("execution-window.json");
        fs::write(&path, &bytes).unwrap();
        f.put(&bytes, true);
        let result = if mode == "resume-command" {
            resume(f.root(), &manifest, &path)
        } else {
            run_with_execution_window(f.root(), &manifest, &path)
        };
        assert!(result.is_err(), "{mode}");
        no_scratch(&f);
    }
}

#[test]
fn registered_workspace_window_enforces_effective_cap_without_changing_recipe_or_policy() {
    let f = Fixture::new();
    let (manifest, _) = prepare(&f, "workspace-window");
    let config_path = f.root().join(".local/storage-rust.json");
    let mut configuration = load(&config_path).unwrap();
    configuration["capacity"]["max_object_bytes"] = json!(16 * 1024 * 1024);
    fs::write(&config_path, configuration.to_string()).unwrap();
    let original = fs::read(&manifest).unwrap();
    let hash = f.put(&original, true);
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("workspace budget"), "{error}");
    let policy = json!({"schema":"reel.native-job-execution-window.v2",
        "job_sha256":hash,"workspace_budget_bytes":32*1024*1024});
    let path = f.root().join("explicit-workspace-policy.json");
    let bytes = serde_json::to_vec(&policy).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(run_with_execution_window(f.root(), &manifest, &path).is_err());
    f.put(&bytes, true);
    let result = run_with_execution_window(f.root(), &manifest, &path).unwrap();
    assert_eq!(result["budget_bytes"], 32 * 1024 * 1024);
    assert_eq!(result["original_budget_bytes"], 16 * 1024 * 1024);
    assert_eq!(result["manifest_sha256"], hash);
    assert_eq!(result["execution_window"]["steps_skipped"], false);
    assert_eq!(
        result["execution_window"]["workspace_budget_explicitly_extended"],
        true
    );
    assert_eq!(fs::read(&manifest).unwrap(), original);
    assert_eq!(
        config(f.root()).unwrap().capacity.max_object_bytes,
        16 * 1024 * 1024
    );
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"source"
    );
    no_scratch(&f);
}

#[test]
fn recovery_workspace_window_restores_only_bounded_metadata_and_preserves_canonical_sources() {
    let f = Fixture::new();
    let (manifest, seed_path, mut seed) = checkpoint_fixture(&f);
    let mut job = load(&manifest).unwrap();
    job["outputs"]["startup"] = json!({"path":"startup.json","max_bytes":1024});
    job["outputs"]["checked-startup"] = json!({"path":"checked-startup.json","max_bytes":1024});
    job["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({"tool":"tools/tool.exe",
        "args":["copy","startup.json","checked-startup.json"],"timeout_seconds":10}));
    let original = serde_json::to_vec(&job).unwrap();
    fs::write(&manifest, &original).unwrap();
    seed["job_sha256"] = json!(f.put(&original, true));
    seed["schema"] = json!("reel.native-job-resume.v3");
    seed["workspace_budget_bytes"] = json!(32 * 1024 * 1024);
    let checkpoint_contents = vec![3_u8; 3 * 1024 * 1024];
    let checkpoint_hash = f.put(&checkpoint_contents, true);
    seed["files"]["prepared/checkpoint.bin"] =
        json!({"sha256":checkpoint_hash,"bytes":checkpoint_contents.len()});
    let metadata = br#"{"schema":"synthetic.startup.v1","tool_checked":true}"#;
    let metadata_hash = f.put(metadata, true);
    seed["metadata_outputs"] = json!({"startup":{"sha256":metadata_hash,"bytes":metadata.len()}});
    let bytes = serde_json::to_vec(&seed).unwrap();
    fs::write(&seed_path, &bytes).unwrap();
    f.put(&bytes, true);
    let result = resume(f.root(), &manifest, &seed_path).unwrap();
    assert_eq!(result["checkpoint_resume"]["first_executed_step"], 1);
    assert_eq!(
        result["checkpoint_resume"]["restored_metadata_output_roles"],
        json!(["startup"])
    );
    assert_eq!(
        result["checkpoint_resume"]["workspace_budget_explicitly_extended"],
        true
    );
    assert_eq!(result["budget_bytes"], 32 * 1024 * 1024);
    assert_eq!(fs::read(&manifest).unwrap(), original);
    assert_eq!(
        fs::read(
            result["outputs"]["checked-startup"]["path"]
                .as_str()
                .unwrap()
        )
        .unwrap(),
        metadata
    );
    assert_eq!(
        fs::read(object(&f.root().join("fast"), &metadata_hash)).unwrap(),
        metadata
    );
    let canonical = seed["files"]["prepared/checkpoint.bin"]["sha256"]
        .as_str()
        .unwrap();
    assert_eq!(
        fs::read(object(&f.root().join("fast"), canonical)).unwrap(),
        checkpoint_contents
    );
    no_scratch(&f);
}

#[test]
fn workspace_window_rejects_legacy_implicit_equal_smaller_and_over_cap_requests() {
    for (schema, budget) in [
        ("reel.native-job-execution-window.v1", 32_u64 * 1024 * 1024),
        ("reel.native-job-execution-window.v2", 16 * 1024 * 1024),
        ("reel.native-job-execution-window.v2", 1),
        (
            "reel.native-job-execution-window.v2",
            17 * 1024 * 1024 * 1024,
        ),
    ] {
        let f = Fixture::new();
        let (manifest, _) = prepare(&f, "copy");
        let hash = f.put(&fs::read(&manifest).unwrap(), true);
        let policy = json!({"schema":schema,"job_sha256":hash,"workspace_budget_bytes":budget});
        let path = f.root().join("invalid-workspace-window.json");
        let bytes = serde_json::to_vec(&policy).unwrap();
        fs::write(&path, &bytes).unwrap();
        f.put(&bytes, true);
        assert!(run_with_execution_window(f.root(), &manifest, &path).is_err());
        no_scratch(&f);
    }
}

#[test]
fn metadata_output_seed_cannot_restore_media_oversized_unknown_or_non_json_outputs() {
    for mode in ["media", "large-bound", "unknown", "invalid-json", "legacy"] {
        let f = Fixture::new();
        let (manifest, path, mut seed) = checkpoint_fixture(&f);
        let mut job = load(&manifest).unwrap();
        job["outputs"]["startup"] = json!({"path":if mode=="media" {"startup.mp4"} else {"startup.json"},
            "max_bytes":if mode=="large-bound" {2*1024*1024} else {1024}});
        let original = serde_json::to_vec(&job).unwrap();
        fs::write(&manifest, &original).unwrap();
        seed["job_sha256"] = json!(f.put(&original, true));
        seed["schema"] = json!(if mode == "legacy" {
            "reel.native-job-resume.v1"
        } else {
            "reel.native-job-resume.v3"
        });
        let contents: &[u8] = if mode == "invalid-json" {
            b"not JSON"
        } else {
            br#"{"ok":true}"#
        };
        let oid = f.put(contents, true);
        let role = if mode == "unknown" {
            "unknown"
        } else {
            "startup"
        };
        seed["metadata_outputs"] = json!({(role):{"sha256":oid,"bytes":contents.len()}});
        let bytes = serde_json::to_vec(&seed).unwrap();
        fs::write(&path, &bytes).unwrap();
        f.put(&bytes, true);
        assert!(resume(f.root(), &manifest, &path).is_err(), "{mode}");
        no_scratch(&f);
    }
}
