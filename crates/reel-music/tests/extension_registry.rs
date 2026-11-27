use std::{collections::BTreeMap, fs};

use reel_music::extensions::{Extension, FilePin, Installation, Registry, SCHEMA};

#[test]
fn verifies_optional_installations_before_exposing_a_command() {
    let folder = tempfile::tempdir().unwrap();
    let binary = folder.path().join("fixture");
    fs::write(&binary, b"exact fixture").unwrap();
    let pin = FilePin {
        path: binary.clone(),
        sha256: reel_music::hash::sha256_bytes(b"exact fixture"),
        bytes: 13,
    };
    let json = serde_json::json!({
        "schema": SCHEMA,
        "installations": {
            "ffmpeg": Installation {
                version: "fixture".into(),
                files: BTreeMap::from([("executable".into(), pin)]),
            }
        }
    });
    let manifest = folder.path().join("registry.json");
    fs::write(&manifest, serde_json::to_vec(&json).unwrap()).unwrap();
    let registry = Registry::load(&manifest).unwrap();
    assert!(registry.resolve(Extension::Demucs).unwrap().is_none());
    assert!(registry.command(Extension::Demucs).is_err());
    assert_eq!(
        registry.command(Extension::Ffmpeg).unwrap().get_program(),
        binary
    );
    fs::write(&binary, b"other fixture").unwrap();
    assert!(registry.command(Extension::Ffmpeg).is_err());
}
