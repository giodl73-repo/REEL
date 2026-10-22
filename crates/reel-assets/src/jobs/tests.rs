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
