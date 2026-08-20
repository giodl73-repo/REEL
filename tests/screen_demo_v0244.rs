use std::{fs, path::Path, process::Command};

use image::{Rgba, RgbaImage};
use reel::screen_demo::write_capture_receipt;
use serde_json::{Value, json};
use tempfile::tempdir;

fn png(path: &Path, color: [u8; 4], width: u32, height: u32) {
    RgbaImage::from_pixel(width, height, Rgba(color))
        .save(path)
        .unwrap();
}

fn fixture(root: &Path) -> (std::path::PathBuf, Value) {
    let state = root.join("state.json");
    fs::write(&state, br#"{"fingerprint":"owner-state"}"#).unwrap();
    let cli = root.join("cli.png");
    let tui = root.join("tui.png");
    let web = root.join("web.png");
    png(&cli, [20, 30, 40, 255], 120, 68);
    png(&tui, [30, 40, 50, 255], 120, 68);
    png(&web, [40, 50, 60, 255], 144, 90);
    let input = json!({
        "schema": "reel.screen-demo-capture-input.v0.1",
        "demo_id": "icelines-matchup-demo",
        "owner_state_ref_sha256": "1".repeat(64),
        "state_document": {
            "file_id": "sealed-matchup-card",
            "path": state
        },
        "required_surfaces": ["cli", "tui", "web"],
        "captures": [
            {
                "capture_id": "cli-card",
                "sequence": 0,
                "surface": "cli",
                "viewport_id": "terminal-120x34",
                "path": cli,
                "width": 120,
                "height": 68
            },
            {
                "capture_id": "tui-card",
                "sequence": 1,
                "surface": "tui",
                "viewport_id": "terminal-120x34",
                "path": tui,
                "width": 120,
                "height": 68
            },
            {
                "capture_id": "web-card",
                "sequence": 2,
                "surface": "web",
                "viewport_id": "desktop-1440x900",
                "path": web,
                "width": 144,
                "height": 90
            }
        ]
    });
    let path = root.join("input.json");
    fs::write(&path, serde_json::to_vec_pretty(&input).unwrap()).unwrap();
    (path, input)
}

#[test]
fn writes_path_free_receipt_for_exact_cross_surface_bytes() {
    let directory = tempdir().unwrap();
    let (input, _) = fixture(directory.path());
    let output = directory.path().join("receipt.json");
    let receipt = write_capture_receipt(&input, &output).unwrap();
    assert_eq!(receipt.schema, "reel.screen-demo-capture-receipt.v0.1");
    assert_eq!(receipt.capture_count, 3);
    assert!(receipt.exact_required_surface_coverage);
    assert!(receipt.capture_bytes_verified);
    assert!(!receipt.capture_state_correspondence_verified);
    assert!(!receipt.capture_semantics_verified);
    assert!(receipt.privacy_review_required);
    assert!(!receipt.redaction_verified);
    assert!(!receipt.accessibility_verified);
    assert!(!receipt.commands_executed_by_reel);
    assert!(!receipt.browser_controlled_by_reel);
    assert!(!receipt.captures_created_by_reel);

    let serialized = fs::read_to_string(output).unwrap();
    assert!(!serialized.contains(&directory.path().display().to_string()));
    assert!(!serialized.contains(".png"));
    assert!(!serialized.contains("owner-state"));
}

#[test]
fn rejects_missing_surfaces_duplicate_bytes_and_wrong_dimensions() {
    let directory = tempdir().unwrap();
    let (input_path, input) = fixture(directory.path());

    let mut missing = input.clone();
    missing["captures"].as_array_mut().unwrap().pop();
    fs::write(&input_path, serde_json::to_vec_pretty(&missing).unwrap()).unwrap();
    assert!(
        write_capture_receipt(&input_path, directory.path().join("missing.json"))
            .unwrap_err()
            .to_string()
            .contains("missing required surfaces: web")
    );

    let (_, mut duplicate) = fixture(directory.path());
    duplicate["captures"][1]["path"] = duplicate["captures"][0]["path"].clone();
    fs::write(&input_path, serde_json::to_vec_pretty(&duplicate).unwrap()).unwrap();
    assert!(
        write_capture_receipt(&input_path, directory.path().join("duplicate.json"))
            .unwrap_err()
            .to_string()
            .contains("aliases another")
    );

    let (_, duplicate_bytes) = fixture(directory.path());
    let cli_path = duplicate_bytes["captures"][0]["path"].as_str().unwrap();
    let tui_path = duplicate_bytes["captures"][1]["path"].as_str().unwrap();
    fs::copy(cli_path, tui_path).unwrap();
    fs::write(
        &input_path,
        serde_json::to_vec_pretty(&duplicate_bytes).unwrap(),
    )
    .unwrap();
    assert!(
        write_capture_receipt(&input_path, directory.path().join("duplicate-bytes.json"))
            .unwrap_err()
            .to_string()
            .contains("duplicates another capture's exact bytes")
    );

    let (_, mut wrong_size) = fixture(directory.path());
    wrong_size["captures"][0]["width"] = json!(121);
    fs::write(&input_path, serde_json::to_vec_pretty(&wrong_size).unwrap()).unwrap();
    assert!(
        write_capture_receipt(&input_path, directory.path().join("wrong-size.json"))
            .unwrap_err()
            .to_string()
            .contains("dimensions mismatch")
    );

    let (_, invalid_png) = fixture(directory.path());
    let web_path = invalid_png["captures"][2]["path"].as_str().unwrap();
    fs::write(web_path, b"not a png").unwrap();
    fs::write(
        &input_path,
        serde_json::to_vec_pretty(&invalid_png).unwrap(),
    )
    .unwrap();
    assert!(
        write_capture_receipt(&input_path, directory.path().join("invalid-png.json"))
            .unwrap_err()
            .to_string()
            .contains("failed to identify screen capture media type")
    );
}

#[test]
fn cli_refuses_to_overwrite_receipt() {
    let directory = tempdir().unwrap();
    let (input, _) = fixture(directory.path());
    let output = directory.path().join("receipt.json");
    let first = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("screen-demo-capture-receipt")
        .arg(&input)
        .arg("--output-path")
        .arg(&output)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let receipt: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(receipt["passed"], true);

    let second = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("screen-demo-capture-receipt")
        .arg(&input)
        .arg("--output-path")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("refusing to overwrite"));
}
