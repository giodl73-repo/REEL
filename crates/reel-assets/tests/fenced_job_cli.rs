#![cfg(windows)]
//! Real executable intake using a synthetic locally compiled fixture only.
use reel_assets::{
    inventory_evidence::sidecar::file_store::collector::{self, fence::Domain},
    versions,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};
fn hash(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
fn tool() -> &'static [u8] {
    static TOOL: OnceLock<Vec<u8>> = OnceLock::new();
    TOOL.get_or_init(|| {
        let temp = tempfile::tempdir().unwrap();
        let exe = temp.path().join("fixture.exe");
        let out = Command::new("rustc")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/job_lifecycle.rs"
            ))
            .arg("-o")
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        fs::read(exe).unwrap()
    })
}
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    manifest: PathBuf,
    fence: PathBuf,
    input: String,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        for name in [".local", "registry", "hot", "warm"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        for name in ["fence", "index-staging", "index-retained"] {
            fs::create_dir(root.join("hot").join(name)).unwrap();
        }
        fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
        fs::write(root.join(".local/storage-rust.json"),json!({"path_policy":"configured-local-roots","cache_roots":[root.join("hot"),root.join("warm")],
            "archive_roots":[root.join("UNAVAILABLE_ARCHIVE")],"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}).to_string()).unwrap();
        fs::write(root.join("asset-project.json"),json!({"schema":"reel.asset-project.v1","project_id":"PRIVATE_JOB_PROJECT",
            "storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"max_inventory_objects":100,"lfs_namespaces":[]}).to_string()).unwrap();
        fs::write(
            root.join("registry/versions.json"),
            serde_json::to_vec(&versions::State {
                coverage_complete: true,
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let input = hash(b"source");
        let exe = hash(tool());
        for (id, data) in [(&input, b"source".as_slice()), (&exe, tool())] {
            let path = reel_assets::object(&root.join("hot"), id);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        }
        fs::write(
            root.join("registry/objects.tsv"),
            format!("{input}\t6\n{exe}\t{}\n", tool().len()),
        )
        .unwrap();
        let manifest = root.join("PRIVATE_JOB_MANIFEST.json");
        fs::write(&manifest,json!({"budget_bytes":32*1024*1024,"scratch_bytes":2*1024*1024,"retain_workspace":true,
            "inputs":{"in.bin":{"sha256":input,"bytes":6}},"tools":{"tool.exe":{"sha256":exe,"bytes":tool().len()}},
            "steps":[{"tool":"tool.exe","args":[mode,"in.bin","out.bin"],"timeout_seconds":30}],
            "outputs":{"PRIVATE_OUTPUT":{"path":"out.bin","max_bytes":1024}}}).to_string()).unwrap();
        let fence = root.join("hot/fence");
        Domain::initialize(&fence).unwrap();
        Self {
            _temp: temp,
            root,
            manifest,
            fence,
            input,
        }
    }
    fn command(&self) -> Command {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_reel-assets"));
        cmd.current_dir(&self.root)
            .args(["--root"])
            .arg(&self.root)
            .arg("run-job-fenced")
            .arg(&self.manifest)
            .arg(&self.fence)
            .env_remove("CAMINOS_TRANSFER_CACHE")
            .stdin(Stdio::null())
            .creation_flags(0x08000000);
        cmd
    }
    fn run(&self) -> Output {
        self.command().output().unwrap()
    }
    fn scope(&self) -> collector::Request {
        let raw =
            serde_json::to_vec(&reel_assets::accounting::report(&self.root).unwrap()).unwrap();
        fs::write(self.root.join("baseline.json"), &raw).unwrap();
        let policy = self.root.join("policy.json");
        fs::write(&policy,json!({"schema":"reel.asset-tier-policy.v1","project_id":"PRIVATE_JOB_PROJECT","baseline":{"sha256":hash(&raw),"bytes":raw.len()},
            "tiers":[{"id":"hot","storage":"active","index":0},{"id":"warm","storage":"active","index":1},{"id":"cold","storage":"archive","index":0}],
            "selected_tier":"hot","history_tier":"warm","cold_tier":"cold","history_versions":[],"tier_pins":{},
            "max_objects":100,"max_plan_seconds":60,"expires_after_seconds":120,"max_batch_bytes":1048576,"max_batch_objects":100}).to_string()).unwrap();
        let cache = self.root.join("hot");
        collector::Request {
            schema: "reel.collector-protection-scope.v1".into(),
            caches: vec![collector::CacheScope {
                cache: cache.clone(),
                staging_parent: cache.join("index-staging"),
                retained_parent: cache.join("index-retained"),
                catalogs: vec![],
                job_parent: Some(cache.join("staging/caminos")),
            }],
            consumers: vec![collector::Consumer {
                root: self.root.clone(),
                policy,
            }],
            limits: Default::default(),
        }
    }
}
#[test]
fn executable_runs_explicitly_and_exports_only_count_summary() {
    let fixture = Fixture::new("copy");
    let out = fixture.run();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["schema"], "reel.fenced-job-summary.v1");
    assert_eq!(result["outputs"], 1);
    assert_eq!(result["workspace_retained"], true);
    assert_eq!(result["removal_authorized"], false);
    for (key, value) in result.as_object().unwrap() {
        assert!(key == "schema" || value.is_number() || value.is_boolean());
    }
    let text = result.to_string();
    for private in [
        "PRIVATE_JOB_PROJECT",
        "PRIVATE_OUTPUT",
        fixture.input.as_str(),
        fixture.root.to_str().unwrap(),
    ] {
        assert!(!text.contains(private));
    }
    assert!(!fixture.root.join("UNAVAILABLE_ARCHIVE").exists());
    let scope = fixture.scope();
    let snapshot = collector::capture(&scope).unwrap();
    assert!(matches!(
        snapshot.classify(&fixture.input).unwrap(),
        collector::Membership::Held(_)
    ));
}
#[test]
fn executable_rejects_execution_options_and_missing_domain_without_initializing() {
    let fixture = Fixture::new("copy");
    let out = fixture.command().arg("--apply").output().unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let control = fixture.fence.join("asset-publication-fence.v1.lock");
    fs::remove_file(&control).unwrap();
    let out = fixture.run();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(!control.exists());
    assert!(!fixture.root.join("hot/staging").exists());
}
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn killed_executable_preserves_input_demand_without_keeping_a_subject_lock() {
    let fixture = Fixture::new("sleep");
    let mut child = ChildGuard(
        fixture
            .command()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let parent = fixture.root.join("hot/staging/caminos");
    let started = Instant::now();
    let workspace = loop {
        if let Ok(entries) = fs::read_dir(&parent)
            && let Some(workspace) = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| p.join("child.pid").exists())
        {
            break workspace;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "job exited before fixture readiness"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    };
    let pid = fs::read_to_string(workspace.join("child.pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    unsafe {
        use windows_sys::Win32::{Foundation::*, System::Threading::*};
        let process = OpenProcess(0x0010_0000 | 0x0001, 0, pid);
        if !process.is_null() {
            let status = WaitForSingleObject(process, 5000);
            if status != WAIT_OBJECT_0 {
                TerminateProcess(process, 1);
                WaitForSingleObject(process, 5000);
            }
            CloseHandle(process);
            assert_eq!(
                status, WAIT_OBJECT_0,
                "fixture descendant survived parent termination"
            );
        }
    }
    assert!(workspace.join("job-publication.json").exists());
    assert!(workspace.join("job.json").exists());
    let domain = Domain::open(&fixture.fence).unwrap();
    domain
        .acquire(
            std::slice::from_ref(&fixture.input),
            collector::fence::Mode::Remove,
            Duration::ZERO,
        )
        .unwrap();
    let snapshot = collector::capture(&fixture.scope()).unwrap();
    assert!(
        matches!(snapshot.classify(&fixture.input).unwrap(),collector::Membership::Held(reasons) if reasons.contains(&collector::Reason::JobWorkspaceInput))
    );
}
