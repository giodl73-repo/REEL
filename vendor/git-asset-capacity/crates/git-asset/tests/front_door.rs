use std::process::Command;
#[test]
fn discovery_names_the_native_backend_without_starting_providers() {
    let out = Command::new(env!("CARGO_BIN_EXE_git-asset"))
        .arg("describe")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["product"], "git-asset");
    assert_eq!(value["backend"], "git-annex");
    assert_eq!(value["automatic_cache_operations"], false);
    assert_eq!(value["native_publication_qualified"], false);
    for bad in ["../query", "query;echo", "not-a-command"] {
        let out = Command::new(env!("CARGO_BIN_EXE_git-asset"))
            .arg(bad)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
    }
}
#[cfg(feature = "schema")]
#[test]
fn front_door_dispatches_only_its_named_sibling_service() {
    let direct = Command::new(env!("CARGO_BIN_EXE_git-asset-query"))
        .arg("describe")
        .output()
        .unwrap();
    let through = Command::new(env!("CARGO_BIN_EXE_git-asset"))
        .args(["query", "describe"])
        .output()
        .unwrap();
    assert!(direct.status.success());
    assert!(through.status.success());
    assert_eq!(direct.stdout, through.stdout);
    assert_eq!(direct.stderr, through.stderr);
}
