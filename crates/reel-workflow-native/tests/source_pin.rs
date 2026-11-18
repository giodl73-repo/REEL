//! The old producer SDK remains separate from this exact qualified intake.
use std::{fs, path::Path};
#[test]
fn qualified_sdk_intake_matches_all_247_original_source_pins() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/tig-workflow");
    let pin: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("source-pin.json")).unwrap()).unwrap();
    assert_eq!(
        pin["source_manifest_sha256"],
        "f89d836d82b1a6240fbe77f4fd53b488ed57ceb303111642be53fd668ffe2cfe"
    );
    let entries = pin["files"].as_array().unwrap();
    assert_eq!(entries.len(), 247);
    for entry in entries {
        let path = entry["path"].as_str().unwrap();
        let bytes = fs::read(root.join(path)).unwrap();
        assert_eq!(
            bytes.len() as u64,
            entry["bytes"].as_u64().unwrap(),
            "source length: {path}"
        );
        assert_eq!(
            reel_workflow::digest(&bytes),
            entry["sha256"].as_str().unwrap(),
            "source digest: {path}"
        );
    }
}
