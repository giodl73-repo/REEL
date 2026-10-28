use super::*;
use tig_core::tree::TreeLimits;
use tig_store::metadata::MetadataReader;

pub(crate) fn profile(f: &Fixture, request: &reel::tig_scene::Request) -> Value {
    setup(f.root(), &f.profile, request)
}
fn setup(root: &Path, source: &Value, request: &reel::tig_scene::Request) -> Value {
    for name in ["capture-capacity", "capture-coordination", "capture-state"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let cache = json!({"cache_roots":[root.join("capture-capacity")],
        "path_policy":"configured-local-roots", "coordination_root":root.join("capture-coordination"),
        "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}});
    let setup = json!({"schema":"reel.tig-job-store-setup.experimental-1",
        "state_parent":root.join("capture-state"),"capacity_cache":root.join("capture-capacity"),
        "cache":cache,"metadata_limits":{"nodes":8192,"bytes":8*1024*1024_u64,
            "database_bytes":16*1024*1024_u64,"min_free_bytes":65536},
        "journal_project":"fixture","journal_epoch":"epoch"});
    let path = root.join("store-setup.json");
    fs::write(&path, serde_json::to_vec(&setup).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .arg("setup")
        .arg(path)
        .arg(root.join("capture-stores.json"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stores: Value =
        serde_json::from_slice(&fs::read(root.join("capture-stores.json")).unwrap()).unwrap();
    json!({"schema":"reel.tig-native-capture-profile.experimental-1",
        "source":source, "request":request, "metadata_root":stores["metadata_root"],
        "metadata_minimum":stores["metadata_minimum"], "metadata_cache":root.join("capture-capacity"),
        "preparation_cache":root.join("capture-capacity"),
        "cache":cache})
}
pub(crate) fn metadata_root(root: &Path) -> PathBuf {
    let stores: Value =
        serde_json::from_slice(&fs::read(root.join("capture-stores.json")).unwrap()).unwrap();
    PathBuf::from(stores["metadata_root"].as_str().unwrap())
}
pub(crate) fn run(f: &Fixture, profile: &Value, name: &str) -> std::process::Output {
    run_at(f.root(), profile, name)
}
fn run_at(root: &Path, profile: &Value, name: &str) -> std::process::Output {
    let input = root.join("capture-profile.json");
    fs::write(&input, serde_json::to_vec(profile).unwrap()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .args(["capture-context"])
        .arg(input)
        .arg(root.join(name))
        .output()
        .unwrap()
}

#[test]
#[ignore = "explicit existing source/profile metadata only; no image/audio decode or runtime adoption"]
fn explicit_existing_source_capture_cli_rehearsal() {
    let profile =
        std::env::var_os("REEL_TIG_SCOPE_PROFILE").expect("explicit source profile required");
    let request = std::env::var_os("REEL_TIG_SCOPE_REQUEST").expect("explicit request required");
    let source = fs::read(profile).unwrap();
    assert!(source.len() <= 65_536);
    let request = reel::tig_scene::Request::read(Path::new(&request)).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let input = setup(
        temp.path(),
        &serde_json::from_slice(&source).unwrap(),
        &request,
    );
    let result = run_at(temp.path(), &input, "context.json");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["readback_equal"], true);
    assert_eq!(summary["media_rendered"], false);
    assert_eq!(summary["job_adopted"], false);
    assert_eq!(
        fs::read_dir(temp.path().join("capture-capacity"))
            .unwrap()
            .count(),
        0
    );
    println!("{summary}");
}
fn clean(f: &Fixture) {
    assert_eq!(
        fs::read_dir(f.root().join("capture-capacity"))
            .unwrap()
            .count(),
        0
    );
    let admissions = f.root().join("capture-coordination/admissions");
    if admissions.exists() {
        assert_eq!(fs::read_dir(admissions).unwrap().count(), 0);
    }
}
fn replay(f: &Fixture, name: &str, output: &str) -> (Value, Value) {
    let fragment: Value = serde_json::from_slice(&fs::read(f.root().join(name)).unwrap()).unwrap();
    assert!(fs::metadata(f.root().join(name)).unwrap().len() <= 65536);
    assert_eq!(fragment["context_mode"], "compiled");
    let minimum = serde_json::from_value(fragment["metadata_minimum"].clone()).unwrap();
    let mut reader = MetadataReader::open(&metadata_root(f.root()), &minimum).unwrap();
    let (_, context) = reel::tig_scene::restore_render_context(
        serde_json::from_value(fragment["source"].clone()).unwrap(),
        &serde_json::from_value(fragment["request"].clone()).unwrap(),
        &serde_json::from_value(fragment["context"].clone()).unwrap(),
        &f.root().join(output),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(context.selection()).unwrap(),
        fragment["context"]
    );
    let job =
        serde_json::from_slice(&fs::read(f.root().join(output).join("delivery/job.json")).unwrap())
            .unwrap();
    (fragment, job)
}

#[test]
fn cli_capture_reuses_provider_tracks_one_leaf_and_replays_both_without_git() {
    let (mut f, request) = native_scene_fixture();
    let mut input = profile(&f, &request);
    let before_parent = fs::read(f.root().join("records/scene.tig")).unwrap();
    let first = run(&f, &input, "first-context.json");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let report: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["readback_equal"], true);
    assert_eq!(report["media_rendered"], false);
    assert_eq!(report["job_adopted"], false);
    assert!(!String::from_utf8_lossy(&first.stdout).contains("scene/native"));
    clean(&f);
    let replacement = picture([0, 255, 0]);
    let pin = Object::from_bytes(&replacement);
    fs::write(
        f.root().join("cache/objects").join(pin.sha256.as_str()),
        replacement,
    )
    .unwrap();
    let mut leaf: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-b.tig")).unwrap()).unwrap();
    leaf["selected"] = json!(pin);
    write(f.root(), "records/cel-b.tig", &leaf);
    git(f.root(), &["add", "records"]);
    git(f.root(), &["commit", "-m", "replace only cel B"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
    input["source"] = f.profile.clone();
    let second = run(&f, &input, "second-context.json");
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    clean(&f);
    assert_eq!(
        fs::read(f.root().join("records/scene.tig")).unwrap(),
        before_parent
    );
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root()
                .join(format!("offline-{}", name.trim_start_matches('.'))),
        )
        .unwrap();
    }
    let (first, old_job) = replay(&f, "first-context.json", "replay-first");
    let (second, new_job) = replay(&f, "second-context.json", "replay-second");
    assert_ne!(first["context"], second["context"]);
    assert_eq!(old_job["pictures"][0], new_job["pictures"][0]);
    assert_ne!(old_job["pictures"][1], new_job["pictures"][1]);
    assert_eq!(old_job["audio"], new_job["audio"]);
    assert_eq!(
        new_job["pictures"][1]["source"]["sha256"],
        json!(pin.sha256)
    );
}

#[test]
fn capture_refusals_preserve_metadata_existing_outputs_and_other_workers() {
    let (f, request) = native_scene_fixture();
    let input = profile(&f, &request);
    let database = metadata_root(f.root()).join("nodes.sqlite");
    let header = metadata_root(f.root()).join("metadata.json");
    let original = (fs::read(&database).unwrap(), fs::read(&header).unwrap());
    let mut low = input.clone();
    low["cache"]["capacity"]["max_object_bytes"] = json!(1);
    assert!(!run(&f, &low, "refused.json").status.success());
    assert!(!f.root().join("refused.json").exists());
    clean(&f);
    fs::write(f.root().join("existing.json"), b"another worker's result").unwrap();
    assert!(!run(&f, &input, "existing.json").status.success());
    assert_eq!(
        fs::read(f.root().join("existing.json")).unwrap(),
        b"another worker's result"
    );
    let mut invalid = input.clone();
    invalid["source"]["commit"] = json!("0".repeat(40));
    let refused = run(&f, &invalid, "bad-source.json");
    assert!(!refused.status.success());
    assert!(!String::from_utf8_lossy(&refused.stderr).contains(&f.root().display().to_string()));
    clean(&f);
    let mut invalid = input.clone();
    invalid["request"]["language"] = json!("");
    assert!(!run(&f, &invalid, "bad-request.json").status.success());
    assert_eq!(fs::read(&database).unwrap(), original.0);
    assert_eq!(fs::read(&header).unwrap(), original.1);
    clean(&f);
}

#[test]
fn concurrent_captures_share_admission_and_retry_without_overwriting_each_other() {
    let (f, request) = native_scene_fixture();
    let input = profile(&f, &request);
    let path = f.root().join("concurrent-capture-profile.json");
    fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
    let sentinel = f.root().join("capture-capacity/other-worker");
    fs::write(&sentinel, b"unrelated worker scratch").unwrap();
    let spawn = |name: &str| {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("capture-context")
            .arg(&path)
            .arg(f.root().join(name))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = spawn("concurrent-a.json");
    let second = spawn("concurrent-b.json");
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    assert!(
        first.status.success() || second.status.success(),
        "both captures refused: {} / {}",
        String::from_utf8_lossy(&first.stderr),
        String::from_utf8_lossy(&second.stderr)
    );
    // A scoped metadata-writer conflict is a retry, not a global render lock.
    for (result, name) in [(first, "concurrent-a.json"), (second, "concurrent-b.json")] {
        if !result.status.success() {
            assert!(!f.root().join(name).exists());
            assert!(run(&f, &input, name).status.success());
        }
    }
    assert_eq!(fs::read(&sentinel).unwrap(), b"unrelated worker scratch");
    fs::remove_file(sentinel).unwrap();
    clean(&f);
    let (first, first_job) = replay(&f, "concurrent-a.json", "concurrent-replay-a");
    let (second, second_job) = replay(&f, "concurrent-b.json", "concurrent-replay-b");
    assert_eq!(first["context"], second["context"]);
    assert_eq!(first_job, second_job);
}
