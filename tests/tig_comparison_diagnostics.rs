#![cfg(feature = "tig-publication")]
use reel::tig_native_job_cli::compare_retained;

#[test]
fn comparison_profile_refusals_hide_locators_and_untrusted_contents() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("private-source-identifier.json");
    let report = root.path().join("private-delivery-identifier.json");
    for bytes in [
        None,
        Some(b"private-invalid-json".as_slice()),
        Some(b"{}".as_slice()),
    ] {
        if let Some(bytes) = bytes {
            std::fs::write(&profile, bytes).unwrap();
        }
        let error = compare_retained(&profile, &report).unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "retained comparison refused at profile admission"
        );
        assert!(!report.exists());
    }
}
