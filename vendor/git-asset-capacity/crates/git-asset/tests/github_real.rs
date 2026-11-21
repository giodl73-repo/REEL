#![cfg(feature = "github")]
//! Explicit hosted qualification: only fresh synthetic stores and an absent
//! random metadata scope. No media, user cache or persistent credential copy.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use git_asset::{
    github,
    lifecycle::{Budget, Enrollment},
};
use git_asset_core::{Digest, LogicalPath, identity, tree::MemoryNodes};
use git_asset_store::{
    Key,
    ledger::CapacityLedger,
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    metadata::{Limits as MetadataLimits, MetadataStore},
    orchestration::Place,
    placement::{self, Authority, Config, Mapping, Provider, Role},
    protection::Limits,
    source_claims::{
        Command as Claim, Kind, Places,
        admission::{Namespace, Policy},
    },
    store_protection::{Binding, Member},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
fn key(value: &str) -> Key {
    Key::parse(value).unwrap()
}
fn hash(v: &impl serde::Serialize) -> Digest {
    Digest::blob(&identity::canonical_json(&serde_json::to_value(v).unwrap()).unwrap())
}
fn env(name: &str) -> String {
    std::env::var(name).expect("hosted fixture context missing")
}
#[test]
#[ignore = "requires a fresh GitHub-hosted OIDC token and trusted workflow context; ordinary ignore is not proof"]
fn actual_hosted_oidc_reaches_foreground_receiver_without_payloads() {
    let token = env("GIT_ASSET_REAL_GITHUB_OIDC");
    assert!(token.len() <= github::MAX_TOKEN);
    let parts = token.split('.').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    let repository = env("GIT_ASSET_REAL_REPOSITORY");
    let repository_id = env("GIT_ASSET_REAL_REPOSITORY_ID");
    let owner_id = env("GIT_ASSET_REAL_OWNER_ID");
    let reference = env("GIT_ASSET_REAL_REF");
    let (owner, name) = repository.split_once('/').unwrap();
    let subjects = [
        format!("repo:{repository}:ref:{reference}"),
        format!("repo:{owner}@{owner_id}/{name}@{repository_id}:ref:{reference}"),
    ];
    let subject = subjects
        .iter()
        .find(|s| claims["sub"].as_str() == Some(s.as_str()))
        .expect("fixture needs one independently constructed default subject")
        .clone();
    let root = tempfile::Builder::new()
        .prefix("git-asset hosted no payload fixture ")
        .tempdir()
        .unwrap();
    let places = Places {
        current: vec![key("working"), key("backup")],
        pending: vec![key("working"), key("backup")],
        predecessor: vec![key("history"), key("backup")],
    };
    let policy = Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|k| (k, places.clone()))
            .collect(),
    };
    let namespace_root = root.path().join("namespace");
    let mut namespace =
        Namespace::create(&namespace_root, key("hosted-fixture"), key("epoch"), policy).unwrap();
    let limits = Limits {
        millis: 60000,
        roots: 32,
        bindings: 1024,
        associations: 2048,
        reads: 10000,
        read_bytes: 4 * 1024 * 1024,
    };
    namespace
        .apply(
            key("enroll"),
            Claim::Enroll {
                source: key("main"),
                owner: key("owner"),
                class: Kind::Branch,
            },
            &MemoryNodes::default(),
            &limits,
        )
        .unwrap();
    let metadata_root = root.path().join("metadata");
    let metadata = MetadataStore::create(
        &metadata_root,
        MetadataLimits {
            nodes: 1000,
            bytes: 1024 * 1024,
            database_bytes: 4 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let metadata_minimum = metadata.checkpoint();
    drop(metadata);
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).unwrap();
    let nonce = Digest::blob(&nonce);
    let enrollment = Enrollment {
        repository: Digest::blob(repository_id.as_bytes()),
        authority: Digest::blob(b"ephemeral hosted fixture authority"),
        namespace: namespace.identity().clone(),
        policy: hash(namespace.policy()),
        source: key("main"),
        owner: key("owner"),
        class: Kind::Branch,
        ref_name: reference.clone(),
        scope: vec![
            LogicalPath::parse(&format!(
                "__git_asset_no_payload_fixture_{}",
                nonce.as_str()
            ))
            .unwrap(),
        ],
        budget: Budget {
            entries: 32,
            nodes: 10000,
            node_bytes: 4 * 1024 * 1024,
            millis: 120000,
        },
    };
    let mut providers = Vec::new();
    let mut roots = Vec::new();
    for (name, role) in [
        ("working", Role::Cache),
        ("history", Role::Cache),
        ("backup", Role::Retained),
    ] {
        let path = root.path().join(name);
        let store = if role == Role::Cache {
            LocalStore::create_managed(
                &path,
                key(name),
                Binding::new(
                    1,
                    Digest::blob(b"fixture clients"),
                    vec![Member {
                        namespace: namespace.identity().clone(),
                        places: vec![key(name)],
                    }],
                )
                .unwrap(),
            )
            .unwrap()
        } else {
            LocalStore::create(&path, key(name)).unwrap()
        };
        let ledger_root = root.path().join(format!("ledger-{name}"));
        let mut ledger = CapacityLedger::create(&ledger_root, key(name), key("epoch"), 0).unwrap();
        LocalCapacityProbe::bind(&store, &mut ledger)
            .unwrap()
            .observe(&store, &mut ledger, key("probe"), key("sample"))
            .unwrap();
        let job = ledger
            .reserve(key("controller"), key("owner"), 128 * 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        providers.push(Provider {
            place: Place::local(&store, 1).unwrap(),
            role,
            failure_domain: key(name),
            capacity: ledger.identity().clone(),
            job,
            assignments: vec![Mapping {
                namespace: namespace.identity().clone(),
                place: key(name),
            }],
        });
        roots.push(json!({"root":path,"ledger_root":ledger_root,"minimum":ledger.checkpoint()}));
    }
    let seed = root.path().join("empty-seed");
    fs::create_dir(&seed).unwrap();
    let byte_source =
        json!({"places":[{"root":seed,"layout":"flat"}],"required":"cooperative-immutable"});
    let config = Config {
        schema: "tig.placement-enrollment.design-1".into(),
        owner: key("owner"),
        authority: enrollment.authority.clone(),
        seed: hash(&byte_source),
        providers,
        namespaces: vec![Authority {
            namespace: namespace.identity().clone(),
            policy: enrollment.policy.clone(),
        }],
        minimum_domains: 1,
        budget: placement::Budget {
            objects: 32,
            max_object_bytes: 1024,
            max_stream_bytes: 1024,
            max_peak_bytes: 128 * 1024 * 1024,
            limits,
        },
    };
    let storage_path = root.path().join("storage.json");
    fs::write(&storage_path,identity::canonical_json(&json!({"schema":"tig.placement-profile.design-1","config":config,"namespaces":[{"root":namespace_root,"minimum":namespace.checkpoint()}],"providers":roots,"byte_source":byte_source})).unwrap()).unwrap();
    let trust = github::Trust {
        schema: "tig.github-trust.design-1".into(),
        audience: env("GIT_ASSET_REAL_AUDIENCE"),
        subject,
        repository,
        repository_id,
        owner_id,
        ref_name: reference,
        workflow_ref: env("GIT_ASSET_REAL_WORKFLOW_REF"),
        reusable: None,
        events: vec![env("GIT_ASSET_REAL_EVENT")],
        max_token_seconds: 600,
        clock_skew_seconds: 30,
        selection_enrollment: enrollment.id().unwrap(),
        placement_enrollment: config.id().unwrap(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let public_keys = github::http::Client::new(None, 30000)
        .unwrap()
        .keys(now, 300)
        .unwrap();
    github::verify(
        &trust,
        &public_keys,
        &token,
        &git_asset::source::GitId::parse(&env("GITHUB_SHA")).unwrap(),
        now,
    )
    .unwrap_or_else(|error| panic!("live issuer verification refused: {error}"));
    let profile = root.path().join("receiver.json");
    fs::write(&profile,identity::canonical_json(&json!({"schema":"tig.github-receiver.design-1","trust":trust,"enrollment":enrollment,"namespace":{"root":namespace_root,"minimum":namespace.checkpoint()},"metadata_root":metadata_root,"metadata_minimum":metadata_minimum,"placement_profile":storage_path,"api_token_env":"GITHUB_TOKEN","listen":"127.0.0.1:0","budget":{"pages":1,"objects":32,"stream_bytes":1024,"millis":180000},"max_connections":1,"allow_receive":true,"allow_selection":true})).unwrap()).unwrap();
    let executable = env!("CARGO_BIN_EXE_git-asset-trigger");
    let mut child = Command::new(executable)
        .arg("serve")
        .arg(profile)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line);
        tx.send((result, line)).unwrap();
    });
    let ready = match rx.recv_timeout(Duration::from_secs(60)) {
        Ok((Ok(_), line)) => line,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("receiver startup did not finish");
        }
    };
    reader.join().unwrap();
    let ready: Value = serde_json::from_str(&ready).unwrap();
    if ready["schema"] != "tig.github-listener.design-1" {
        let _ = child.kill();
        let _ = child.wait();
        panic!("receiver startup refused");
    }
    let mut stream = TcpStream::connect(ready["listening"].as_str().unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(240)))
        .unwrap();
    let body =
        json!({"schema":"tig.github-request.design-1","commit":env("GITHUB_SHA")}).to_string();
    write!(stream,"POST /v1/reconcile HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
    let mut response = String::new();
    stream
        .take(1024 * 1024)
        .read_to_string(&mut response)
        .unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(!response.contains(&token));
    let value: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    let code = match value["code"].as_str() {
        Some(
            "invalid_request" | "budget" | "authority" | "unauthorized" | "stale" | "unavailable"
            | "disabled",
        ) => value["code"].as_str().unwrap(),
        _ => "no_static_error_code",
    };
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "receiver rejection: {code}"
    );
    assert_eq!(value["authenticated"], true);
    assert_eq!(value["result"]["storage_converged"], true);
    assert_eq!(value["result"]["objects"], 0);
    assert_eq!(value["result"]["stream_bytes"], 0);
    assert_eq!(value["result"]["effects"], 0);
    println!(
        "PASS live issuer/foreground receiver with synthetic empty scope; no payload operations"
    );
}
