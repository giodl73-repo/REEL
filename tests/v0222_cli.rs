use std::process::Command;

const FIXTURE: &str = "manifests/fixtures/chapter-score/manifest.yaml";

fn successful(binary: &str, args: &[&str]) -> String {
    let output = Command::new(binary)
        .args(args)
        .output()
        .expect("command runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn cli_validates_and_compiles_chapter_score_direction() {
    let binary = env!("CARGO_BIN_EXE_reel");
    let validation = successful(binary, &["validate", FIXTURE, "--output", "json"]);
    assert!(validation.contains("\"score_cues\": 3"));

    let json = successful(binary, &["score-plan", FIXTURE, "--output", "json"]);
    assert!(json.contains("\"schema\": \"reel.score-plan.v0.1\""));
    assert!(json.contains("\"chapter\": \"Desert\""));
    assert!(json.contains("\"family\": \"hand-percussion\""));
    assert!(json.contains("preserving complete scoring calls"));

    let text = successful(binary, &["score-plan", FIXTURE]);
    assert!(text.contains("score_cues=3"));
    assert!(text.contains("desert-lift | chapter=Desert"));
}
