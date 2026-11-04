#![cfg(feature = "print")]
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn pin(path: &Path) -> serde_json::Value {
    let bytes = fs::read(path).unwrap();
    json!({"path":path,"bytes":bytes.len(),"sha256":Sha256::digest(&bytes).iter().map(|b|format!("{b:02x}")).collect::<String>()})
}
#[test]
fn real_print_cli_keeps_private_paths_titles_ids_hashes_and_text_out_of_stdout() {
    let root = tempfile::tempdir().unwrap();
    let (font, family) = [
        ("C:/Windows/Fonts/arial.ttf", "Arial"),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "DejaVu Sans",
        ),
        (
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
            "Liberation Sans",
        ),
    ]
    .into_iter()
    .find(|(path, _)| Path::new(path).is_file())
    .expect("explicit fixture font required");
    let source = root.path().join("private-source.txt");
    fs::write(&source, "Private source phrase with canción").unwrap();
    let request = json!({"schema":"reel.print-collection.v1","title":"Private collection title","author":"Private source creator","notice":"Private technical fixture","private_only":true,"fonts":[pin(Path::new(font))],"body_font":family,"entries":[{"kind":"text","id":"private-record-id","title":"Private chapter title","source":pin(&source)}],"limits":{"input_bytes":16777216,"output_bytes":16777216,"pages":8}});
    let mut request = request;
    request["entries"][0]["source"]["path"] = json!("private-source.txt");
    let path = root.path().join("request.json");
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    let output = root.path().join("packet");
    for operation in ["plan", "render", "check"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reel-print"));
        command
            .current_dir(root.path())
            .args([operation, "request.json"]);
        if operation != "plan" {
            command.arg(&output);
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["pages"], 2);
        if operation == "plan" {
            assert!(!output.exists());
        } else {
            assert_eq!(
                value["output_files"].as_u64().unwrap(),
                fs::read_dir(&output).unwrap().count() as u64
            );
            assert_eq!(
                value["output_bytes"].as_u64().unwrap(),
                fs::read_dir(&output)
                    .unwrap()
                    .map(|p| p.unwrap().metadata().unwrap().len())
                    .sum::<u64>()
            );
        }
        let stdout = String::from_utf8(result.stdout).unwrap();
        for secret in [
            "Private",
            "private-record-id",
            "private-source.txt",
            font,
            request["entries"][0]["source"]["sha256"].as_str().unwrap(),
        ] {
            assert!(!stdout.contains(secret));
        }
    }
}
