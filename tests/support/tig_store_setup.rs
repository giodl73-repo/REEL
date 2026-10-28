use super::*;

fn fixture() -> (tempfile::TempDir, Value) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for name in ["state", "capacity", "coordination"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let profile = json!({"schema":"reel.tig-job-store-setup.experimental-1",
        "state_parent":root.join("state"),"capacity_cache":root.join("capacity"),
        "cache":{"cache_roots":[root.join("capacity")],"coordination_root":root.join("coordination"),
            "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}},
        "metadata_limits":{"nodes":8192,"bytes":8*1024*1024_u64,"database_bytes":16*1024*1024_u64,
            "min_free_bytes":65536},"journal_project":"fixture","journal_epoch":"epoch"});
    (temp, profile)
}
fn setup(root: &Path, profile: &Value, name: &str) -> std::process::Output {
    let input = root.join("setup-profile.json");
    fs::write(&input, serde_json::to_vec(profile).unwrap()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .arg("setup")
        .arg(input)
        .arg(root.join(name))
        .output()
        .unwrap()
}
fn check(root: &Path, name: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .arg("check-stores")
        .arg(root.join(name))
        .output()
        .unwrap()
}
fn value(root: &Path, name: &str) -> Value {
    serde_json::from_slice(&fs::read(root.join(name)).unwrap()).unwrap()
}
fn released(root: &Path) {
    let records = root.join("coordination/admissions");
    if records.exists() {
        assert_eq!(records.read_dir().unwrap().count(), 0);
    }
    assert_eq!(root.join("capacity").read_dir().unwrap().count(), 0);
}

#[test]
fn explicit_setup_publishes_reopened_stores_and_preserves_existing_bindings() {
    let (temp, profile) = fixture();
    let root = temp.path();
    fs::write(root.join("state/unrelated"), b"another worker").unwrap();
    let created = setup(root, &profile, "stores.json");
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let report: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(report["readback_equal"], true);
    assert_eq!(report["job_adopted"], false);
    assert!(!String::from_utf8_lossy(&created.stdout).contains(&root.display().to_string()));
    let original = fs::read(root.join("stores.json")).unwrap();
    let stores = value(root, "stores.json");
    for key in ["metadata_root", "journal_root"] {
        assert!(Path::new(stores[key].as_str().unwrap()).starts_with(root.join("state")));
    }
    let checked = check(root, "stores.json");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["metadata_nodes"], 0);
    assert_eq!(checked["journal_commands"], 0);
    assert_eq!(checked["authorizes_eviction"], false);
    assert!(!setup(root, &profile, "stores.json").status.success());
    assert_eq!(fs::read(root.join("stores.json")).unwrap(), original);
    assert_eq!(root.join("state").read_dir().unwrap().count(), 2);
    assert_eq!(
        fs::read(root.join("state/unrelated")).unwrap(),
        b"another worker"
    );
    released(root);
}

#[test]
fn setup_refuses_capacity_bad_bounds_and_foreign_or_missing_provider_checkpoints() {
    let (temp, profile) = fixture();
    let root = temp.path();
    for kind in ["capacity", "bounds", "coordination"] {
        let mut invalid = profile.clone();
        match kind {
            "capacity" => invalid["cache"]["capacity"]["max_object_bytes"] = json!(1),
            "bounds" => invalid["metadata_limits"]["nodes"] = json!(0),
            _ => invalid["cache"]["coordination_root"] = json!(root.join("missing-coordination")),
        }
        let result = setup(root, &invalid, "refused.json");
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stderr).contains(&root.display().to_string()));
        assert!(!root.join("refused.json").exists());
        assert_eq!(root.join("state").read_dir().unwrap().count(), 0);
        released(root);
    }
    assert!(!root.join("missing-coordination").exists());
    assert!(setup(root, &profile, "first.json").status.success());
    assert!(setup(root, &profile, "second.json").status.success());
    let mut foreign = value(root, "first.json");
    foreign["metadata_minimum"] = value(root, "second.json")["metadata_minimum"].clone();
    fs::write(
        root.join("foreign.json"),
        serde_json::to_vec(&foreign).unwrap(),
    )
    .unwrap();
    assert!(!check(root, "foreign.json").status.success());
    foreign = value(root, "first.json");
    foreign["journal_minimum"] = value(root, "second.json")["journal_minimum"].clone();
    fs::write(
        root.join("foreign.json"),
        serde_json::to_vec(&foreign).unwrap(),
    )
    .unwrap();
    assert!(!check(root, "foreign.json").status.success());
    let mut missing = value(root, "first.json");
    missing["metadata_root"] = json!(root.join("missing-metadata"));
    fs::write(
        root.join("missing.json"),
        serde_json::to_vec(&missing).unwrap(),
    )
    .unwrap();
    assert!(!check(root, "missing.json").status.success());
    assert!(!root.join("missing-metadata").exists());
    assert!(check(root, "first.json").status.success());
    assert!(check(root, "second.json").status.success());
    released(root);
}

#[test]
fn concurrent_setup_same_binding_publishes_one_complete_authority_and_cleans_loser() {
    let (temp, profile) = fixture();
    let root = temp.path();
    let input = root.join("setup-profile.json");
    fs::write(&input, serde_json::to_vec(&profile).unwrap()).unwrap();
    let spawn = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("setup")
            .arg(&input)
            .arg(root.join("stores.json"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = spawn();
    let second = spawn();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    assert_ne!(first.status.success(), second.status.success());
    assert_eq!(root.join("state").read_dir().unwrap().count(), 1);
    assert!(check(root, "stores.json").status.success());
    released(root);
}
