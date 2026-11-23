#![cfg(feature = "github")]
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use git_asset::github::{self, Error};
use git_asset_core::Digest;
use serde_json::json;
#[path = "common/github_fixture.rs"]
mod fixture;
use fixture::{Fixture, canonical};
#[test]
fn independent_signature_yields_exact_nonserializable_admission_and_redacted_evidence() {
    let f = Fixture::new();
    let token = f.token(&f.claims);
    let v = github::verify(&f.trust, &f.keys, &token, &f.commit, 1001).unwrap();
    assert_eq!(v.commit(), &f.commit);
    v.admit(
        1001,
        &f.trust.selection_enrollment,
        &f.trust.placement_enrollment,
    )
    .unwrap();
    assert_eq!(
        v.admit(1001, &Digest::blob(b"other"), &f.trust.placement_enrollment),
        Err(Error::Trust)
    );
    assert_eq!(
        v.admit(
            999,
            &f.trust.selection_enrollment,
            &f.trust.placement_enrollment
        ),
        Err(Error::Clock)
    );
    assert_eq!(
        v.admit(
            1300,
            &f.trust.selection_enrollment,
            &f.trust.placement_enrollment
        ),
        Err(Error::Expired)
    );
    assert_eq!(
        v.admit(
            1200,
            &f.trust.selection_enrollment,
            &f.trust.placement_enrollment
        ),
        Err(Error::Clock)
    );
    let report = serde_json::to_string(v.provenance()).unwrap();
    assert!(!report.contains(&token));
    assert!(!report.contains("synthetic-token-id"));
    assert!(!format!("{v:?}").contains("synthetic-token-id"));
}
#[test]
fn signature_payload_and_key_substitution_cannot_authenticate() {
    let f = Fixture::new();
    let token = f.token(&f.claims);
    let mut pieces = token.split('.').map(str::to_owned).collect::<Vec<_>>();
    let mut claims = f.claims.clone();
    claims["repository_id"] = json!("9999");
    pieces[1] = URL_SAFE_NO_PAD.encode(canonical(&claims));
    assert_eq!(
        github::verify(&f.trust, &f.keys, &pieces.join("."), &f.commit, 1001).unwrap_err(),
        Error::Signature
    );
    let mut keys = f.keys.clone();
    keys.keys[0].kid = "different-key".into();
    assert_eq!(
        github::verify(&f.trust, &keys, &token, &f.commit, 1001).unwrap_err(),
        Error::Key
    );
    let other = Fixture::new();
    assert_eq!(
        github::verify(&f.trust, &other.keys, &token, &f.commit, 1001).unwrap_err(),
        Error::Signature
    );
}
#[test]
fn signed_wrong_repository_ref_event_runner_and_workflow_are_not_authority() {
    let f = Fixture::new();
    for (field, value) in [
        ("iss", "https://untrusted.invalid"),
        ("aud", "other-audience"),
        ("sub", "other-subject"),
        ("repository", "other/repo"),
        ("repository_id", "999"),
        ("repository_owner_id", "999"),
        ("ref", "refs/heads/draft"),
        ("ref_type", "tag"),
        ("event_name", "pull_request"),
        ("runner_environment", "self-hosted"),
        (
            "workflow_ref",
            "fixture-owner/fixture/.github/workflows/other.yml@refs/heads/main",
        ),
        ("workflow_sha", "not-a-sha"),
        ("run_id", "0"),
        ("run_attempt", "01"),
    ] {
        let mut c = f.claims.clone();
        c[field] = json!(value);
        assert_eq!(f.verify(&c, 1001).unwrap_err(), Error::Claims, "{field}");
    }
    let mut c = f.claims.clone();
    c["sha"] = json!("c".repeat(40));
    assert_eq!(f.verify(&c, 1001).unwrap_err(), Error::Stale);
    for field in ["aud", "repository_id", "exp"] {
        let mut c = f.claims.clone();
        c[field] = json!(["wrong shape"]);
        assert_eq!(f.verify(&c, 1001).unwrap_err(), Error::Claims);
    }
}
#[test]
fn time_and_key_validity_are_bounded_and_exact_expiry_refuses() {
    let f = Fixture::new();
    assert_eq!(f.verify(&f.claims, 1300).unwrap_err(), Error::Expired);
    assert_eq!(f.verify(&f.claims, 990).unwrap_err(), Error::Clock);
    for (field, value) in [("exp", 1000), ("exp", 1601), ("iat", 1100), ("nbf", 1100)] {
        let mut c = f.claims.clone();
        c[field] = json!(value);
        assert_eq!(f.verify(&c, 1001).unwrap_err(), Error::Clock, "{field}");
    }
    let mut keys = f.keys.clone();
    keys.valid_until = 1001;
    assert_eq!(
        github::verify(&f.trust, &keys, &f.token(&f.claims), &f.commit, 1001).unwrap_err(),
        Error::Expired
    );
    keys = f.keys.clone();
    keys.keys.push(keys.keys[0].clone());
    assert_eq!(keys.validate(1001), Err(Error::Key));
}
#[test]
fn issuer_backdated_not_before_does_not_change_issuance_or_expiry_limits() {
    let f = Fixture::new();
    let mut claims = f.claims.clone();
    claims["nbf"] = json!(940);
    assert!(f.verify(&claims, 1001).is_ok());
    assert_eq!(f.verify(&claims, 990).unwrap_err(), Error::Clock);
    assert_eq!(f.verify(&claims, 1300).unwrap_err(), Error::Expired);
    claims["exp"] = json!(1601);
    assert_eq!(f.verify(&claims, 1001).unwrap_err(), Error::Clock);
}
#[test]
fn duplicate_claims_and_unknown_jose_or_algorithm_mechanisms_refuse() {
    let f = Fixture::new();
    for header in [
        json!({"alg":"HS256","typ":"JWT","kid":"fixture-key"}),
        json!({"alg":"none","typ":"JWT","kid":"fixture-key"}),
        json!({"alg":"RS256","typ":"JWT","kid":"fixture-key","jku":"https://untrusted.invalid/keys"}),
        json!({"alg":"RS256","typ":"JWT","kid":"fixture-key","crit":["b64"],"b64":false}),
    ] {
        let t = f.signed(&header, &canonical(&f.claims));
        assert!(matches!(
            github::verify(&f.trust, &f.keys, &t, &f.commit, 1001),
            Err(Error::Invalid)
        ));
    }
    let bytes = canonical(&f.claims);
    let mut duplicate = bytes[..bytes.len() - 1].to_vec();
    duplicate.extend_from_slice(b",\"exp\":1300}");
    let t = f.signed(
        &json!({"alg":"RS256","typ":"JWT","kid":"fixture-key"}),
        &duplicate,
    );
    assert_eq!(
        github::verify(&f.trust, &f.keys, &t, &f.commit, 1001).unwrap_err(),
        Error::Invalid
    );
    let token = f.token(&f.claims);
    let padded = token.replacen('.', "=.", 1);
    assert!(github::verify(&f.trust, &f.keys, &padded, &f.commit, 1001).is_err());
    assert_eq!(
        github::verify(
            &f.trust,
            &f.keys,
            &"x".repeat(github::MAX_TOKEN + 1),
            &f.commit,
            1001
        )
        .unwrap_err(),
        Error::Budget
    );
}
#[test]
fn reusable_workflow_pin_and_unknown_authenticated_claims_have_distinct_semantics() {
    let mut f = Fixture::new();
    f.trust.reusable = Some(github::Reusable {
        workflow_ref: format!(
            "fixture-owner/automation/.github/workflows/git-asset.yml@{}",
            "d".repeat(40)
        ),
        workflow_sha: "d".repeat(40),
    });
    assert_eq!(f.verify(&f.claims, 1001).unwrap_err(), Error::Claims);
    let mut c = f.claims.clone();
    let r = f.trust.reusable.as_ref().unwrap();
    c["job_workflow_ref"] = json!(r.workflow_ref);
    c["job_workflow_sha"] = json!(r.workflow_sha);
    c["future_provider_claim"] = json!({"ignored":true});
    f.verify(&c, 1001).unwrap();
    c["job_workflow_sha"] = json!("e".repeat(40));
    assert_eq!(f.verify(&c, 1001).unwrap_err(), Error::Claims);
}

#[test]
fn receiver_cli_discovery_and_default_disabled_startup_never_open_machine_roots() {
    use git_asset::lifecycle::{Budget, Enrollment};
    use git_asset_core::{LogicalPath, identity};
    use git_asset_store::{
        Key,
        metadata::{Limits as MetadataLimits, MetadataStore},
        source_claims::{
            Kind, Places,
            admission::{Namespace, Policy},
        },
    };
    use std::{fs, process::Command};
    let executable = env!("CARGO_BIN_EXE_git-asset-trigger");
    let described = Command::new(executable).arg("describe").output().unwrap();
    assert!(described.status.success());
    assert!(described.stderr.is_empty());
    let described: serde_json::Value = serde_json::from_slice(&described.stdout).unwrap();
    assert_eq!(described["default_receive"], false);
    assert_eq!(described["loopback_only"], true);
    assert_eq!(described["installs_service"], false);
    let mut f = Fixture::new();
    let key = |s: &str| Key::parse(s).unwrap();
    let places = Places {
        current: vec![key("working")],
        pending: vec![key("working")],
        predecessor: vec![key("history")],
    };
    let policy = Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|k| (k, places.clone()))
            .collect(),
    };
    let ns = Namespace::create(
        &f.root.path().join("namespace-fixture"),
        key("project"),
        key("epoch"),
        policy,
    )
    .unwrap();
    let metadata = MetadataStore::create(
        &f.root.path().join("metadata-fixture"),
        MetadataLimits {
            nodes: 1000,
            bytes: 1024 * 1024,
            database_bytes: 4 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let enrollment = Enrollment {
        repository: Digest::blob(b"repo"),
        authority: Digest::blob(b"owner authority"),
        namespace: ns.identity().clone(),
        policy: Digest::blob(
            &identity::canonical_json(&serde_json::to_value(ns.policy()).unwrap()).unwrap(),
        ),
        source: key("main"),
        owner: key("owner"),
        class: Kind::Branch,
        ref_name: f.trust.ref_name.clone(),
        scope: vec![LogicalPath::parse("assets").unwrap()],
        budget: Budget {
            entries: 32,
            nodes: 1000,
            node_bytes: 1024 * 1024,
            millis: 10000,
        },
    };
    f.trust.selection_enrollment = enrollment.id().unwrap();
    let never = f.root.path().join("must-not-open");
    let mut profile = json!({"schema":"tig.github-receiver.design-1","trust":f.trust,"enrollment":enrollment,"namespace":{"root":never.join("namespace"),"minimum":ns.checkpoint()},"metadata_root":never.join("metadata"),"metadata_minimum":metadata.checkpoint(),"placement_profile":never.join("storage.json"),"api_token_file":never.join("private-api-token"),"listen":"127.0.0.1:0","budget":{"pages":1,"objects":32,"stream_bytes":1024,"millis":10000},"max_connections":1});
    let path = f.root.path().join("receiver.json");
    fs::write(&path, canonical(&profile)).unwrap();
    let output = Command::new(executable)
        .arg("serve")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["code"],
        "disabled"
    );
    assert!(!never.exists());
    profile["allow_receive"] = json!(true);
    profile["listen"] = json!("0.0.0.0:0");
    fs::write(&path, canonical(&profile)).unwrap();
    let output = Command::new(executable)
        .arg("serve")
        .arg(path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["code"],
        "invalid"
    );
    assert!(!never.exists());
}
