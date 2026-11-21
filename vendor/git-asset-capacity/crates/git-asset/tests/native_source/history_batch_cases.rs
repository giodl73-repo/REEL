use super::*;

fn fixture() -> CommandFixture {
    let mut f = CommandFixture::new();
    let other = f.source.record("other", "other/2", false, json!([]), None);
    let base = commit(f.source.root());
    f.profile["schema"] = json!("tig.history-batch-profile.experimental-1");
    f.profile["base"] = json!(base.as_str());
    let expected = f
        .profile
        .as_object_mut()
        .unwrap()
        .remove("expected")
        .unwrap();
    f.profile.as_object_mut().unwrap().remove("asset");
    f.profile["proposal"]
        .as_object_mut()
        .unwrap()
        .remove("path");
    let raw = manifest(
        "other/2",
        "other-a",
        &Object::from_bytes(b"other bytes"),
        &[],
    );
    fs::write(f.temp.path().join("other.json"), &raw).unwrap();
    let other_request = json!({"operation":"register-revision","coverage":"partial",
        "document":{"path":"other.json","expected":Object::from_bytes(&raw)}});
    f.request = json!({"schema":"tig.history-batch-request.experimental-1","items":[
        {"asset":"item/1","path":"records/item.asset","expected":expected,"request":f.request},
        {"asset":"other/2","path":"records/other.asset","expected":other,"request":other_request}]});
    f
}
fn apply(f: &mut CommandFixture, result: &Value) {
    git(
        f.source.root(),
        &[
            "cherry-pick",
            result["proposal"]["commit"].as_str().unwrap(),
        ],
    );
    f.profile["base"] = json!(head(f.source.root()).as_str());
    for (item, result) in f.request["items"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(result["items"].as_array().unwrap())
    {
        item["expected"] = result["record"].clone();
    }
}

#[test]
fn multi_asset_batch_one_snapshot_sparse_proposal_reordering_retry_and_noop() {
    let mut f = fixture();
    let before = f.state();
    let check = f.ok("batch-check");
    assert_eq!(f.state(), before);
    assert_eq!(check["items"].as_array().unwrap().len(), 2);
    let proposed = f.ok("batch-propose");
    assert_eq!(proposed["new_metadata_readback_verified"], true);
    let changed = git(
        f.source.root(),
        &[
            "diff",
            "--name-only",
            f.profile["base"].as_str().unwrap(),
            proposed["proposal"]["commit"].as_str().unwrap(),
        ],
    );
    assert_eq!(
        String::from_utf8(changed.stdout).unwrap(),
        "records/item.asset\nrecords/other.asset\n"
    );
    let operations = operation_files(&f.root);
    f.request["items"].as_array_mut().unwrap().reverse();
    let reordered = f.ok("batch-propose");
    assert_eq!(reordered["proposal"], proposed["proposal"]);
    assert_eq!(operation_files(&f.root), operations);
    apply(&mut f, &reordered);
    let before = f.state();
    let noop = f.ok("batch-propose");
    assert_eq!(noop["noop"], true);
    assert_eq!(noop["published"], false);
    assert!(noop["proposal"].is_null());
    assert_eq!(f.state(), before);
    let noop = git_asset::history_command::execute::<LocalStore>(
        "batch-propose",
        &f.temp.path().join("profile.json"),
        &f.temp.path().join("request.json"),
        |_| panic!("all-noop batch must not open a publisher"),
    )
    .unwrap();
    assert_eq!(noop["noop"], true);
    let repository = reader(f.source.root());
    let snapshot = capture(&repository, head(f.source.root()));
    for id in ["item/1", "other/2"] {
        let inspection = snapshot
            .inspect_history(
                id,
                Dialect::Draft202012,
                revisions::Limits::default(),
                &mut read_cache(&f.root),
            )
            .unwrap();
        assert_eq!(inspection.summary().revisions, 1);
    }
}

#[test]
fn multi_asset_batch_late_invalid_candidate_never_opens_a_provider() {
    let mut f = fixture();
    f.request["items"][1]["request"] = json!({"operation":"select-revision",
        "revision":Object::from_bytes(b"unregistered"),"representation":"main",
        "subject":Object::from_bytes(b"other bytes"),"status":{},"streams":[]});
    f.refuse("batch-propose", "history preparation:");
    let error = git_asset::history_command::execute::<LocalStore>(
        "batch-propose",
        &f.temp.path().join("profile.json"),
        &f.temp.path().join("request.json"),
        |_| panic!("late validation failure must not open a publisher"),
    )
    .unwrap_err();
    assert!(error.contains("history preparation:"));
}

#[test]
fn multi_asset_batch_identity_scope_and_stale_path_refuse_even_without_a_proposal() {
    let mut f = fixture();
    let original = f.request.clone();
    f.request["items"][1]["asset"] = json!("item/1");
    f.refuse("batch-propose", "duplicate or invalid batch identity/path");
    f.request = original.clone();
    f.request["items"][1]["path"] = json!("records/item.asset");
    f.refuse("batch-propose", "duplicate or invalid batch identity/path");
    f.request = original.clone();
    f.request["items"][1]["path"] = json!("outside/other.asset");
    f.refuse("batch-propose", "batch path outside declared record scope");
    f.request = original;
    f.profile.as_object_mut().unwrap().remove("proposal");
    f.request["items"][0]["path"] = json!("records/other.asset");
    f.request["items"][1]["path"] = json!("records/item.asset");
    f.refuse("batch-publish", "stale batch path");
}

#[test]
fn multi_asset_batch_request_budgets_refuse_before_publication() {
    let mut f = fixture();
    let item = f.request["items"][0].clone();
    f.request["items"] = json!([]);
    f.refuse("batch-propose", "history batch item budget");
    f.request["items"] = json!(vec![item.clone(); 65]);
    f.refuse("batch-propose", "history batch item budget");
    let mut repeated = item.clone();
    repeated["request"] =
        json!({"operation":"sequence","changes":vec![item["request"].clone();64]});
    f.request["items"] = json!(vec![repeated; 5]);
    f.refuse("batch-propose", "history batch operation budget");
}

#[test]
fn multi_asset_batch_preflights_later_object_capabilities_before_any_intent_write() {
    let f = fixture();
    let path = f.temp.path().join("other.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["representations"] = json!(
        (0..32)
            .map(|n| json!({"key":format!("repr-{n}-{}", "x".repeat(100)),
        "object":Object::from_bytes(b"other bytes")}))
            .collect::<Vec<_>>()
    );
    let raw = serde_json::to_vec(&manifest).unwrap();
    fs::write(&path, &raw).unwrap();
    let mut f = f;
    f.request["items"][1]["request"]["document"]["expected"] = json!(Object::from_bytes(&raw));
    f.ok("batch-check");
    let before = f.state();
    let error = git_asset::history_command::execute(
        "batch-propose",
        &f.temp.path().join("profile.json"),
        &f.temp.path().join("request.json"),
        |_| {
            Ok(Fault {
                inner: LocalStore::open(&f.root).unwrap(),
                commit: 0,
                fail: usize::MAX,
                after: false,
                max_bytes: Some(4096),
            })
        },
    )
    .unwrap_err();
    assert!(error.contains("publication preflight: Budget"), "{error}");
    assert_eq!(f.state(), before);
}

#[test]
fn multi_asset_batch_partial_publication_retains_acks_and_retries_without_a_git_proposal() {
    let f = fixture();
    f.ok("batch-check");
    let before = f.state();
    let error = git_asset::history_command::execute(
        "batch-propose",
        &f.temp.path().join("profile.json"),
        &f.temp.path().join("request.json"),
        |_| {
            Ok(Fault {
                inner: LocalStore::open(&f.root).unwrap(),
                commit: 0,
                fail: 5,
                after: true,
                max_bytes: None,
            })
        },
    )
    .unwrap_err();
    assert!(error.contains("history publication: Provider"));
    assert!(!error.contains("private-provider-path"));
    let after = f.state();
    assert_eq!(before.0, after.0);
    assert_eq!(before.1, after.1);
    assert!(after.2 > 0 && after.3 > 0);
    let result = f.ok("batch-propose");
    assert_eq!(result["new_metadata_readback_verified"], true);
    let operations = operation_files(&f.root);
    assert_eq!(f.ok("batch-propose")["proposal"], result["proposal"]);
    assert_eq!(operation_files(&f.root), operations);
}

#[test]
fn multi_asset_batch_wrong_readback_place_preserves_publication_then_exact_retry_succeeds() {
    let mut f = fixture();
    f.ok("batch-check");
    let before = f.state();
    let actual = f.temp.path().join("actual-batch-provider");
    LocalStore::create(&actual, key("actual-batch-provider")).unwrap();
    let error = git_asset::history_command::execute(
        "batch-propose",
        &f.temp.path().join("profile.json"),
        &f.temp.path().join("request.json"),
        |_| LocalStore::open(&actual).map_err(|_| publication::Error::Provider),
    )
    .unwrap_err();
    assert!(
        error.contains("published metadata readback: Missing"),
        "{error}"
    );
    assert_eq!(f.state(), before);
    let operations = operation_files(&actual);
    assert!(operations > 0);
    f.profile["publication"]["root"] = json!(actual);
    f.profile["byte_source"]["places"][0]["root"] = json!(actual);
    let result = f.ok("batch-propose");
    assert_eq!(result["new_metadata_readback_verified"], true);
    assert_eq!(operation_files(&actual), operations);
}

#[test]
fn multi_asset_batch_full_sixty_four_item_boundary_uses_one_sparse_proposal() {
    let mut f = fixture();
    let mut items = f.request["items"].as_array().unwrap().clone();
    for n in 2..64 {
        let asset = format!("batch/{n}");
        let file = format!("batch-{n}");
        let expected = f.source.record(&file, &asset, n % 2 == 0, json!([]), None);
        let raw = manifest(
            &asset,
            "imported-a",
            &Object::from_bytes(b"shared synthetic bytes"),
            &[],
        );
        let document = format!("batch-{n}.json");
        fs::write(f.temp.path().join(&document), &raw).unwrap();
        items.push(
            json!({"asset":asset,"path":format!("records/{file}.asset"),"expected":expected,
            "request":{"operation":"register-revision","coverage":"partial",
                "document":{"path":document,"expected":Object::from_bytes(&raw)}}}),
        );
    }
    f.profile["base"] = json!(commit(f.source.root()).as_str());
    f.request["items"] = json!(items);
    let original = head(f.source.root());
    let checked = f.ok("batch-check");
    assert_eq!(checked["items"].as_array().unwrap().len(), 64);
    let result = f.ok("batch-propose");
    assert_eq!(head(f.source.root()), original);
    assert_eq!(result["new_metadata_readback_verified"], true);
    let changed = git(
        f.source.root(),
        &[
            "diff",
            "--name-only",
            original.as_str(),
            result["proposal"]["commit"].as_str().unwrap(),
        ],
    );
    assert_eq!(
        String::from_utf8(changed.stdout).unwrap().lines().count(),
        64
    );
}
