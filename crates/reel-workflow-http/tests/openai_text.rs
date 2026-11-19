use reel_workflow::{Artifact, ErrorKind};
use reel_workflow_http::openai_text::{Format, Input, Spec};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn spec() -> Spec {
    Spec {
        schema: "reel.openai-text.v1".into(),
        format: Format::Text,
        output: "candidate.txt".into(),
        max_output_tokens: 2048,
    }
}
fn request(s: &Spec) -> Vec<u8> {
    s.request(
        "declared-model",
        "Conserva el texto.",
        &BTreeMap::new(),
        None,
        65536,
    )
    .unwrap()
}
fn reply(text: &str) -> Value {
    json!({"object":"response","id":"resp_fixture","model":"reported-model-alias","status":"completed","error":null,"incomplete_details":null,"output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]}]})
}
#[test]
fn stateless_request_preserves_text_and_only_exact_explicit_inputs() {
    let raw = "¡Mira la luz!\n".as_bytes();
    let a = Artifact {
        logical_id: "source".into(),
        sha256: reel_workflow::digest(raw),
        bytes: raw.len() as u64,
        media_type: "text/plain".into(),
    };
    let s = spec();
    let inputs = BTreeMap::from([(
        "source".into(),
        Input {
            artifact: &a,
            bytes: raw,
        },
    )]);
    let req = s
        .request("declared-model", "Conserva el texto.", &inputs, None, 65536)
        .unwrap();
    let body: Value = serde_json::from_slice(&req).unwrap();
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], false);
    assert_eq!(body["background"], false);
    assert_eq!(body["tool_choice"], "none");
    assert_eq!(body["tools"], json!([]));
    assert_eq!(body["instructions"], "Conserva el texto.");
    let inner: Value =
        serde_json::from_str(body["input"][0]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(inner["text"], std::str::from_utf8(raw).unwrap());
    assert_eq!(inner["artifact"]["sha256"], a.sha256);
    let out = s
        .normalize(
            "declared-model",
            &req,
            &serde_json::to_vec(&reply("  ¡Sí!\n")).unwrap(),
            4096,
        )
        .unwrap();
    assert_eq!(out["candidate.txt"], "  ¡Sí!\n".as_bytes());
    let meta: Value = serde_json::from_slice(&out["text.json"]).unwrap();
    assert_eq!(meta["requested_model"], "declared-model");
    assert_eq!(meta["reported_model"], "reported-model-alias");
    assert_eq!(meta["approved"], false);
    let bad = BTreeMap::from([(
        "source".into(),
        Input {
            artifact: &a,
            bytes: b"different",
        },
    )]);
    assert_eq!(
        s.request("declared-model", "prompt", &bad, None, 65536)
            .unwrap_err()
            .kind,
        ErrorKind::Integrity
    );
    assert_eq!(
        s.request("declared-model", "prompt", &inputs, None, 10)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
}
#[test]
fn incomplete_refusal_tools_and_multiple_messages_cannot_become_candidates() {
    let s = spec();
    let req = request(&s);
    let mut variants = vec![];
    let mut r = reply("ok");
    r["status"] = json!("incomplete");
    variants.push(r);
    let mut r = reply("ok");
    r["output"][1]["content"][0] = json!({"type":"refusal","refusal":"declined"});
    variants.push(r);
    let mut r = reply("ok");
    r["output"][0] = json!({"type":"function_call","name":"unrequested"});
    variants.push(r);
    let mut r = reply("ok");
    let extra = r["output"][1].clone();
    r["output"].as_array_mut().unwrap().push(extra);
    variants.push(r);
    variants.push(reply(""));
    for r in variants {
        assert!(
            s.normalize(
                "declared-model",
                &req,
                &serde_json::to_vec(&r).unwrap(),
                4096
            )
            .is_err()
        );
    }
    assert_eq!(
        s.normalize(
            "declared-model",
            &req,
            &serde_json::to_vec(&reply("too long")).unwrap(),
            2
        )
        .unwrap_err()
        .kind,
        ErrorKind::Budget
    );
}
#[test]
fn review_format_is_bound_to_original_copy_and_keeps_native_semantics_required() {
    use reel_workflow::review::{Brief, Report};
    let mut s = spec();
    s.format = Format::ReviewReport;
    s.output = "report.json".into();
    assert!(
        s.request("declared-model", "review", &BTreeMap::new(), None, 65536)
            .is_err()
    );
    let report = Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: "a".repeat(64),
        findings: vec![],
    };
    let brief = Brief {
        schema: "reel.workflow-review-brief.v1".into(),
        task_id: "review".into(),
        task_key: "b".repeat(64),
        attempt_id: "original".into(),
        language: "es".into(),
        production_commit: "c".repeat(40),
        review_artifact: Artifact {
            logical_id: "review/png".into(),
            sha256: "d".repeat(64),
            bytes: 32,
            media_type: "image/png".into(),
        },
        report_output: "report.json".into(),
        instructions: "Review supplied image only".into(),
        return_schema: json!({}),
        empty_report: report.clone(),
        units: BTreeMap::new(),
        evidence: vec![],
    };
    let req = s
        .request(
            "declared-model",
            "review",
            &BTreeMap::new(),
            Some(&brief),
            65536,
        )
        .unwrap();
    let body: Value = serde_json::from_slice(&req).unwrap();
    assert_eq!(body["text"]["format"]["strict"], true);
    let exact = serde_json::to_string(&report).unwrap();
    let out = s
        .normalize(
            "declared-model",
            &req,
            &serde_json::to_vec(&reply(&exact)).unwrap(),
            4096,
        )
        .unwrap();
    assert_eq!(out["report.json"], exact.as_bytes());
    let stale = exact.replace(&"a".repeat(64), &"e".repeat(64));
    assert_eq!(
        s.normalize(
            "declared-model",
            &req,
            &serde_json::to_vec(&reply(&stale)).unwrap(),
            4096
        )
        .unwrap_err()
        .kind,
        ErrorKind::Stale
    );
}
#[test]
fn endpoint_policy_and_input_format_refuse_before_transport() {
    let s = spec();
    let mut http = reel_workflow_http::Config {
        schema: "reel.workflow-http.v1".into(),
        endpoint: "https://api.openai.com/v1/responses".into(),
        timeout_seconds: 1,
        request_byte_limit: 65536,
        response_byte_limit: 65536,
        secret_headers: BTreeMap::new(),
        allow_loopback_http: false,
    };
    assert_eq!(s.check_endpoint(&http).unwrap_err().kind, ErrorKind::Denied);
    http.secret_headers.insert(
        "authorization".into(),
        reel_workflow_http::SecretHeader {
            variable: "KEY".into(),
            prefix: "Bearer ".into(),
        },
    );
    s.check_endpoint(&http).unwrap();
    for url in [
        "https://elsewhere.test/v1/responses",
        "https://api.openai.com/v1/responses?x=1",
        "http://localhost/v1/responses",
        "https://api.openai.com/v1/chat/completions",
    ] {
        http.endpoint = url.into();
        assert!(s.check_endpoint(&http).is_err());
    }
    http.endpoint = "http://127.0.0.1:9/v1/responses".into();
    http.allow_loopback_http = true;
    s.check_endpoint(&http).unwrap();
    let raw = b"opaque";
    let a = Artifact {
        logical_id: "opaque".into(),
        sha256: reel_workflow::digest(raw),
        bytes: raw.len() as u64,
        media_type: "video/mp4".into(),
    };
    assert_eq!(
        s.request(
            "declared-model",
            "review",
            &BTreeMap::from([(
                "opaque".into(),
                Input {
                    artifact: &a,
                    bytes: raw
                }
            )]),
            None,
            65536
        )
        .unwrap_err()
        .kind,
        ErrorKind::Unsupported
    );
}
