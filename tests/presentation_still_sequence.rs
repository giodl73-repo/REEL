use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn reference(root: &Path, name: &str) -> serde_json::Value {
    let bytes = fs::read(root.join(name)).unwrap();
    let sha = hash(&bytes);
    json!({"path":name,"sha256":sha,"bytes":bytes.len(),
        "cache_uri":format!("cache://sha256/{sha}")})
}

fn run(root: &Path, action: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_reel-presentation-still-sequence"))
        .arg(action)
        .arg(root.join("manifest.json"))
        .arg("--root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("render"))
        .output()
        .unwrap()
}

#[test]
fn two_stills_render_exact_picture_and_audio_clocks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (name, color) in [("red.png", "red"), ("blue.png", "blue")] {
        let status = Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i"])
            .arg(format!("color=c={color}:s=64x64:r=24:d=1"))
            .args(["-frames:v", "1"])
            .arg(root.join(name))
            .status()
            .unwrap();
        assert!(status.success());
    }
    fs::write(
        root.join("template.json"),
        serde_json::to_vec(&json!({
            "schema":"reel.presentation-still-template.v1",
            "template_id":"fixture","width":64,"height":64,
            "frame_rate":24,"sample_rate":48000,
            "picture_count":2,"frames_per_picture":24
        }))
        .unwrap(),
    )
    .unwrap();
    let ass = [
        "[Script Info]",
        "ScriptType: v4.00+",
        "PlayResX: 64",
        "PlayResY: 64",
        "[V4+ Styles]",
        "Format: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding",
        "Style: Default,Arial,12,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,2,2,2,1",
        "[Events]",
        "Format: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text",
        "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,Caption",
    ]
    .join("\n");
    fs::write(root.join("layer.ass"), ass).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&json!({
            "schema":"reel.presentation-still-sequence.v1",
            "episode_id":"fixture-episode","language":"es","role":"captions",
            "template":reference(root,"template.json"),
            "pictures":[reference(root,"red.png"),reference(root,"blue.png")],
            "editable_layer":reference(root,"layer.ass")
        }))
        .unwrap(),
    )
    .unwrap();
    let built = run(root, "build");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let checked = run(root, "check");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("render/receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["frames"], 48);
    assert_eq!(receipt["samples"], 96_000);
    fs::write(root.join("render/editable-layer.ass"), b"tampered").unwrap();
    assert!(!run(root, "check").status.success());
}
