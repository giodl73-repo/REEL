use std::process::Command;

const PLAN: &str = "manifests/fixtures/showrunner/showrunner.yaml";

fn successful(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(args)
        .output()
        .expect("showrunner command runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn cli_exposes_all_showrunner_commands() {
    assert!(
        successful(&["showrunner-validate", PLAN, "--output", "json"])
            .contains("\"full_coverage\": true")
    );
    assert!(
        successful(&["showrunner-audit", PLAN, "--output", "json"])
            .contains("\"finding_count\": 0")
    );
    assert!(
        successful(&["showrunner-revelation-map", PLAN, "--output", "json"])
            .contains("\"id\": \"place-threshold\"")
    );
    assert!(
        successful(&["showrunner-rhythm-audit", PLAN, "--output", "json"])
            .contains("\"intimate\": 1")
    );
    assert!(
        successful(&["showrunner-rhythm-audit", PLAN, "--output", "text"])
            .contains("Authored internal tone turns: 2")
    );
    assert!(
        successful(&["showrunner-review-queue", PLAN, "--output", "json"])
            .contains("\"open_reviewers\"")
    );
    assert!(
        successful(&["showrunner-review-pack", PLAN, "--output", "text"])
            .starts_with("# Showrunner audit")
    );
    assert!(
        successful(&["showrunner-review-pack", PLAN, "--output", "json"])
            .contains("\"schema\": \"reel.showrunner-review-pack.v0.1\"")
    );
}
