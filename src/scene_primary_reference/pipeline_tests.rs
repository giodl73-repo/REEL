use super::*;

#[test]
fn deadline_stops_owned_pipeline_child() {
    #[cfg(windows)]
    let command = {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        command
    };
    #[cfg(not(windows))]
    let command = {
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        command
    };
    let began = Instant::now();
    assert!(
        run(
            vec![command],
            1024,
            Instant::now() + Duration::from_millis(100)
        )
        .is_err()
    );
    assert!(began.elapsed() < Duration::from_secs(5));
}
#[test]
fn oversized_diagnostic_child() {
    if std::env::var_os("REEL_TEST_REFERENCE_OVERSIZED_CHILD").is_some() {
        print!("{}", "x".repeat(128 * 1024));
    }
}
#[test]
fn oversized_diagnostics_refuse_without_retaining_unbounded_output() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "scene_primary_reference::pipeline::tests::oversized_diagnostic_child",
            "--exact",
            "--nocapture",
        ])
        .env("REEL_TEST_REFERENCE_OVERSIZED_CHILD", "1");
    let error = run(
        vec![command],
        1024,
        Instant::now() + Duration::from_secs(10),
    )
    .err()
    .unwrap();
    assert!(
        error.to_string().contains("diagnostic budget exceeded"),
        "{error:#}"
    );
}
