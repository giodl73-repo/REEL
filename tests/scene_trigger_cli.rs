use sha2::{Digest, Sha256};
use std::{fs, process::Command};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn compiles_phrase_text_without_authored_seconds_and_pins_input_bytes() {
    let root = tempfile::tempdir().unwrap();
    let take = "a".repeat(64);
    let spoken = "The countdown began. Bertha marked the wall.";
    let evidence = serde_json::json!({
        "schema":"reel.scene-word-timing-evidence.v1", "language":"en", "cue_id":"cue",
        "selected_take_sha256":take,"sample_rate":24000,"cue_end_sample":24000,
        "words":[
            {"word":"The","start_sample":100,"end_sample":1000},
            {"word":"countdown","start_sample":2000,"end_sample":3000},
            {"word":"began.","start_sample":4000,"end_sample":5000},
            {"word":"Bertha","start_sample":10000,"end_sample":11000},
            {"word":"marked","start_sample":12000,"end_sample":13000},
            {"word":"the","start_sample":14000,"end_sample":15000},
            {"word":"wall.","start_sample":16000,"end_sample":17000}
        ]
    });
    let spec = serde_json::json!({
        "schema":"reel.scene-trigger-text.v1", "language":"en", "cue_id":"cue",
        "selected_take_sha256":take,"spoken_text":spoken,
        "spoken_text_sha256":hash(spoken.as_bytes()),
        "markers":[{"id":"start"},{"id":"wall","phrase":"Bertha marked the wall"}]
    });
    let evidence_bytes = serde_json::to_vec(&evidence).unwrap();
    let spec_bytes = serde_json::to_vec(&spec).unwrap();
    fs::write(root.path().join("words.json"), &evidence_bytes).unwrap();
    fs::write(root.path().join("triggers.json"), &spec_bytes).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_reel-scene-authoring"))
        .arg("resolve-trigger-text")
        .arg(root.path().join("words.json"))
        .arg(root.path().join("triggers.json"))
        .arg("--alignment")
        .arg(root.path().join("alignment.json"))
        .arg("--receipt")
        .arg(root.path().join("receipt.json"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let alignment_bytes = fs::read(root.path().join("alignment.json")).unwrap();
    let alignment: serde_json::Value = serde_json::from_slice(&alignment_bytes).unwrap();
    assert_eq!(alignment["semantic_markers"]["start"], 0);
    assert_eq!(alignment["semantic_markers"]["wall"], 10000);
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["word_evidence_sha256"], hash(&evidence_bytes));
    assert_eq!(receipt["trigger_spec_sha256"], hash(&spec_bytes));
    assert_eq!(receipt["native_alignment_sha256"], hash(&alignment_bytes));
    assert_eq!(receipt["publication"], "not-authorized");
}
