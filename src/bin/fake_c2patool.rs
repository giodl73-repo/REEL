//! Test-only fixture that emulates the official `c2patool` executable.
//!
//! This binary is gated behind REEL's `test-fixtures` feature and is not part
//! of the shipped workflow. It emits current c2patool-style validation reports
//! selected by the first line of the snapshotted asset.

use std::io::Write;

const VALID: &str = r#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{"format":"image/png","title":"asset","claim_generator":"unknown"}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[{"code":"claimSignature.insideValidity"},{"code":"claimSignature.validated"},{"code":"assertion.dataHash.match"}],"informational":[{"code":"timeStamp.untrusted"}],"failure":[]},"ingredientDeltas":[]}}"#;
const EXPECTED_SETTINGS: &str = r#"{"core":{"decode_identity_assertions":false,"allowed_network_hosts":[],"allow_redirects":false},"verify":{"verify_trust":false,"verify_timestamp_trust":false,"ocsp_fetch":false,"remote_manifest_fetch":false}}"#;

fn argument_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn target_arg(args: &[String]) -> Option<&str> {
    let mut skip_value = false;
    for arg in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg == "--settings" {
            skip_value = true;
            continue;
        }
        if !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version") {
        println!("fake-c2patool 9.9.9");
        return;
    }
    let target = match target_arg(&args) {
        Some(path) => path,
        None => {
            eprintln!("fake-c2patool: no target supplied");
            std::process::exit(2);
        }
    };
    let settings = argument_value(&args, "--settings")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    if settings != EXPECTED_SETTINGS {
        eprintln!("fake-c2patool: network-denied verifier settings were not supplied");
        std::process::exit(4);
    }
    let content = std::fs::read_to_string(target).unwrap_or_default();
    let scenario = content.lines().next().unwrap_or("").trim();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match scenario {
        "valid" => {
            let _ = out.write_all(VALID.as_bytes());
        }
        "trusted" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Trusted","validation_results":{"activeManifest":{"success":[],"informational":[],"failure":[]}}}"#,
            );
        }
        "untrusted" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[{"code":"signingCredential.trusted"}],"informational":[{"code":"timeStamp.untrusted"}],"failure":[]}}}"#,
            );
        }
        "null-status" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_status":null}"#,
            );
        }
        "unknown-status" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[{"code":"future.validation.success"}],"informational":[],"failure":[]}}}"#,
            );
        }
        "duplicate" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[],"informational":[],"failure":[]}}}"#,
            );
        }
        "overlong-label" => {
            let label = "a".repeat(201);
            let _ = write!(
                out,
                r#"{{"active_manifest":"{label}","manifests":{{"{label}":{{}}}},"validation_state":"Valid","validation_results":{{"activeManifest":{{"success":[],"informational":[],"failure":[]}}}}}}"#
            );
        }
        "many-manifests" => {
            let manifests = (0..257)
                .map(|index| format!(r#""manifest-{index}":{{}}"#))
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(
                out,
                r#"{{"active_manifest":"manifest-0","manifests":{{{manifests}}},"validation_state":"Valid","validation_results":{{"activeManifest":{{"success":[],"informational":[],"failure":[]}}}}}}"#
            );
        }
        "extension" => {
            if std::path::Path::new(target)
                .extension()
                .is_some_and(|extension| extension == "mp4")
            {
                let _ = out.write_all(VALID.as_bytes());
            } else {
                let _ = out.write_all(
                    br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[],"informational":[],"failure":[{"code":"assertion.dataHash.mismatch"}]}}}"#,
                );
            }
        }
        "failure" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid","validation_results":{"activeManifest":{"success":[{"code":"claimSignature.validated"}],"informational":[],"failure":[{"code":"future.validation.failure"}]}}}"#,
            );
        }
        "xca-invalid" | "invalid" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Invalid","validation_results":{"activeManifest":{"success":[],"informational":[],"failure":[{"code":"claimSignature.mismatch"}]}}}"#,
            );
        }
        "missing-state" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_results":{"activeManifest":{"success":[],"informational":[],"failure":[]}}}"#,
            );
        }
        "missing-results" => {
            let _ = out.write_all(
                br#"{"active_manifest":"urn:uuid:1","manifests":{"urn:uuid:1":{}},"validation_state":"Valid"}"#,
            );
        }
        "missing" => {
            let _ = out.write_all(br#"{"active_manifest":null,"manifests":{}}"#);
        }
        "malformed" => {
            let _ = out.write_all(b"this is not json {");
        }
        "oversize" => {
            let big = "a".repeat(3_000_000);
            let _ = out.write_all(big.as_bytes());
        }
        "toolfail" => {
            eprintln!("fake-c2patool: simulated failure");
            std::process::exit(3);
        }
        _ => {
            let _ = out.write_all(VALID.as_bytes());
        }
    }
}
