use reel_workflow::ErrorKind;
use reel_workflow_http::{
    Config,
    github_issue::{Body, SCHEMA, Spec},
};
use serde_json::json;
use std::collections::BTreeMap;

fn spec() -> Spec {
    let identity = "1".repeat(64);
    let key = "2".repeat(64);
    Spec {
        schema: SCHEMA.into(),
        repository: "private/film".into(),
        finding_identity: identity.clone(),
        draft_key: key.clone(),
        issue: Body {
            title: "Review picture: C100".into(),
            body: format!(
                "Reported synthetic finding\n> El mar.\n\nDraft version: `{key}`\n\n<!-- reel-film-finding:{identity} -->\n"
            ),
        },
    }
}
fn config() -> Config {
    Config {
        schema: "reel.workflow-http.v1".into(),
        endpoint: "https://api.github.com/repos/private/film/issues".into(),
        timeout_seconds: 5,
        request_byte_limit: 65536,
        response_byte_limit: 65536,
        secret_headers: BTreeMap::new(),
        allow_loopback_http: false,
    }
}
fn response(s: &Spec) -> Vec<u8> {
    serde_json::to_vec(&json!({"number":17,"url":"https://api.github.com/repos/private/film/issues/17","html_url":"https://github.com/private/film/issues/17","title":s.issue.title,"body":s.issue.body,"extra_server_metadata":true})).unwrap()
}
#[test]
fn exported_draft_adapter_checks_semantic_subject_before_preparing_an_issue() {
    use reel_workflow::film_review::{Finding, IssueDraft};
    let s = spec();
    let artifact =
        json!({"logical_id":"review","sha256":"a".repeat(64),"bytes":10,"media_type":"video/mp4"});
    let mut d: IssueDraft=serde_json::from_value(json!({"schema":"reel.workflow-film-issue-draft.v1",
        "key":s.draft_key,"finding_identity":s.finding_identity,"finding_id":"finding-1",
        "project":"fixture","source_commit":"b".repeat(40),"language":"es",
        "review":artifact,"map":artifact,"receipt":artifact,
        "target":{"attachment_id":"picture-1","story_id":"story-1","cel_id":"C100","semantic_id":"event-1",
            "selected_picture":{"path":"picture.png","sha256":"c".repeat(64),"bytes":10},"start_frame":0,"end_frame":24},
        "frame":1,"category":"picture","method":"human-report","observation":"El mar.","evidence":[],
        "title":s.issue.title,"body":s.issue.body,"viewing_verified":false,"authority_verified":false,
        "native_retention_verified":false,"issue_submitted":false,"selection_applied":false})).unwrap();
    let finding = Finding {
        id: d.finding_id.clone(),
        target_id: d.target.attachment_id.clone(),
        frame: d.frame,
        category: d.category,
        method: d.method,
        observation: d.observation.clone(),
        evidence: d.evidence.clone(),
        board_reference: None,
    };
    let identity =
        reel_workflow::canonical_digest(&(&d.review, &d.map, &d.receipt, &d.target, &d.finding_id))
            .unwrap();
    let key =
        reel_workflow::canonical_digest(&(&d.review, &d.map, &d.receipt, &d.target, &finding))
            .unwrap();
    d.body = d
        .body
        .replace(&d.finding_identity, &identity)
        .replace(&d.key, &key);
    d.finding_identity = identity;
    d.key = key;
    let spec = Spec::from_draft("private/film".into(), &d).unwrap();
    assert_eq!(spec.issue.body, d.body);

    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("draft.json");
    let bytes = serde_json::to_vec_pretty(&d).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let pin = reel_workflow::digest(&bytes);
    let prepare = |pin: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow-http"))
            .args(["prepare-github-issue", "private/film"])
            .arg(&path)
            .arg(pin)
            .output()
            .unwrap()
    };
    let prepared = prepare(&pin);
    assert!(prepared.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&prepared.stdout).unwrap(),
        json!({"kind": "github-issue", "spec": spec})
    );
    std::fs::write(&path, [&bytes[..], b" "].concat()).unwrap();
    let changed = prepare(&pin);
    assert!(!changed.status.success());
    let error: serde_json::Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_eq!(error["error"]["code"], "github-issue-draft-pin");
    d.frame = 2;
    assert!(Spec::from_draft("private/film".into(), &d).is_err());
    d.frame = 1;
    d.authority_verified = true;
    assert!(Spec::from_draft("private/film".into(), &d).is_err());
}
#[test]
fn logical_finding_fences_note_version_config_and_repository_case() {
    let mut s = spec();
    let operation = s.operation_id().unwrap();
    let original = s.body(&config()).unwrap();
    s.draft_key = "3".repeat(64);
    s.issue.body = s
        .issue
        .body
        .replace(&"2".repeat(64), &s.draft_key)
        .replace("El mar.", "La ola.");
    assert_eq!(s.operation_id().unwrap(), operation);
    assert_ne!(s.body(&config()).unwrap(), original);
    s.repository = "Private/Film".into();
    assert_eq!(s.operation_id().unwrap(), operation);
    s.finding_identity = "4".repeat(64);
    s.issue.body = s.issue.body.replace(&"1".repeat(64), &s.finding_identity);
    assert_ne!(s.operation_id().unwrap(), operation);
}
#[test]
fn endpoint_and_marker_validation_refuse_before_any_network_or_intent() {
    let mut c = config();
    let s = spec();
    for endpoint in [
        "https://api.github.com/repos/other/film/issues",
        "https://evil.test/repos/private/film/issues",
        "https://api.github.com/repos/private/film/issues?extra=1",
        "http://api.github.com/repos/private/film/issues",
    ] {
        c.endpoint = endpoint.into();
        assert_eq!(s.body(&c).unwrap_err().kind, ErrorKind::Denied);
    }
    c.endpoint = "http://127.0.0.1:1234/repos/private/film/issues".into();
    assert!(s.body(&c).is_err());
    c.allow_loopback_http = true;
    assert!(s.body(&c).is_ok());
    let mut s = spec();
    s.issue.body.push_str("changed");
    assert_eq!(s.operation_id().unwrap_err().kind, ErrorKind::Invalid);
    let mut s = spec();
    s.repository = "private/../film".into();
    assert!(s.operation_id().is_err());
}
#[test]
fn complete_response_requires_exact_issue_subject_and_does_not_grant_native_authority() {
    let s = spec();
    let raw = response(&s);
    let receipt = s.interpret(&config(), 201, &raw).unwrap();
    assert_eq!(receipt["issue_submitted"], true);
    for key in [
        "viewing_verified",
        "authority_verified",
        "native_retained",
        "task_completed",
        "selection_applied",
    ] {
        assert_eq!(receipt[key], false);
    }
    assert_eq!(receipt["response_sha256"], reel_workflow::digest(&raw));
    for status in [200, 202, 301, 401, 403, 409, 422, 429, 500, 503] {
        assert!(s.interpret(&config(), status, &raw).is_err());
    }
    for (key, value) in [
        ("number", json!(0)),
        ("html_url", json!("https://evil.test/17")),
        (
            "url",
            json!("https://api.github.com/repos/other/film/issues/17"),
        ),
        ("body", json!("different")),
        ("title", json!("different")),
        ("pull_request", json!({})),
    ] {
        let mut changed: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        changed[key] = value;
        assert!(
            s.interpret(&config(), 201, &serde_json::to_vec(&changed).unwrap())
                .is_err()
        );
    }
    assert_eq!(
        s.interpret(&config(), 201, b"partial{").unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
}
