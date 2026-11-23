//! Metadata only. No real endpoint ID or playback/capture enrollment.
#![cfg(feature = "installation-native")]
use serde_json::json;
use std::{fs, process::Command};
fn tool() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reel-installation"))
}
fn binding() -> serde_json::Value {
    json!({"profile":"reel.installation.win11-wasapi-pcm24.v1","device_id":"wasapi:{0.0.0.00000000}.{11111111-1111-1111-1111-111111111111}",
    "os_build":26300,"driver_version":"10.0.test-only","configuration_sha256":"a".repeat(64)})
}
#[test]
fn binding_shape_validation_is_private_and_needs_no_device() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("binding.json");
    fs::write(&path, serde_json::to_vec(&binding()).unwrap()).unwrap();
    let output = tool().arg("validate-binding").arg(&path).output().unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["binding_shape_checked"], true);
    assert_eq!(report["endpoint_probed"], false);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("11111111") && !text.contains("test-only"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    for id in ["", "default", "wasapi:default", "alsa:hw:0,0"] {
        let mut value = binding();
        value["device_id"] = json!(id);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            !tool()
                .arg("validate-binding")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
    }
}
#[test]
fn malformed_binding_and_existing_private_output_refuse_without_probe() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("binding.json");
    let mut value = binding();
    value["device_playback_checked"] = json!(true);
    fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !tool()
            .arg("validate-binding")
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    fs::write(&input, vec![b' '; 4097]).unwrap();
    assert!(
        !tool()
            .arg("validate-binding")
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let output = root.path().join("keep.json");
    fs::write(&output, b"keep original").unwrap();
    assert!(
        !tool()
            .arg("probe")
            .arg(root.path().join("does-not-exist.json"))
            .arg("--output")
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read(output).unwrap(), b"keep original");
}
#[test]
fn wrong_runtime_pin_refuses_before_native_query_and_creates_no_final_output() {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("probe.json");
    let output = root.path().join("new.json");
    let value = json!({"schema":"reel.installation-endpoint-probe.v1","endpoint_id":binding()["device_id"],
        "runtime":{"path":env!("CARGO_BIN_EXE_reel-installation"),"sha256":"b".repeat(64),"bytes":1}});
    fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !tool()
            .arg("probe")
            .arg(&request)
            .arg("--output")
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    assert!(!output.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
