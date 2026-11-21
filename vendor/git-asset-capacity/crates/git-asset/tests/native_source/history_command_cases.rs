use super::*;
use git_asset_core::identity;

#[path = "history_batch_cases.rs"]
mod batch_cases;

struct CommandFixture {
    source: Fixture,
    temp: TempDir,
    root: std::path::PathBuf,
    profile: Value,
    request: Value,
    document: Vec<u8>,
}

#[test]
fn sequence_command_registers_and_selects_with_one_readback_and_sparse_proposal() {
    let mut f = CommandFixture::new();
    let register = f.request.clone();
    let revision = Object::from_bytes(&f.document);
    f.request = json!({"operation":"sequence","changes":[register,
        {"operation":"select-revision","revision":revision,"representation":"main",
        "subject":Object::from_bytes(b"not copied media"),"status":{"editorial":"draft"},"streams":[]}]});
    let before = f.state();
    let check = f.ok("check");
    assert_eq!(f.state(), before);
    assert_eq!(check["metadata"].as_array().unwrap().len(), 2);
    let proposal = f.ok("propose");
    assert_eq!(proposal["new_metadata_readback_verified"], true);
    assert_eq!(check["record"], proposal["record"]);
    assert_eq!(f.ok("propose")["proposal"], proposal["proposal"]);
    assert_eq!(
        head(f.source.root()).as_str(),
        f.profile["base"].as_str().unwrap()
    );
    f.apply(&proposal);
    let record: Value =
        serde_json::from_slice(&fs::read(f.source.root().join("records/item.asset")).unwrap())
            .unwrap();
    assert_eq!(record["status"]["editorial"], "draft");
    let before = f.state();
    let noop = f.ok("propose");
    assert_eq!(noop["noop"], true);
    assert_eq!(noop["published"], false);
    assert!(noop["proposal"].is_null());
    assert_eq!(f.state(), before);
    let noop = git_asset::history_command::execute::<LocalStore>(
        "propose",
        &f.temp.path().join("profile.json"),
        &f.temp.path().join("request.json"),
        |_| panic!("sequence no-op must not construct a publisher"),
    )
    .unwrap();
    assert_eq!(noop["noop"], true);
}

#[test]
fn sequence_command_rejects_late_failure_and_shape_before_opening_a_provider() {
    let mut f = CommandFixture::new();
    let register = f.request.clone();
    f.request = json!({"operation":"sequence","changes":[register.clone(),
        {"operation":"select-revision","revision":Object::from_bytes(b"missing"),"representation":"main",
        "subject":Object::from_bytes(b"not copied media"),"status":{},"streams":[]}]});
    f.refuse("propose", "history preparation:");
    // Materialize explicit inputs through check, then exercise the shared host seam.
    f.run("check");
    let result = git_asset::history_command::execute::<LocalStore>(
        "propose",
        &f.temp.path().join("profile.json"),
        &f.temp.path().join("request.json"),
        |_| panic!("a late sequence failure must never open a publisher"),
    );
    assert!(result.is_err());
    for changes in [
        json!([]),
        json!([{"operation":"sequence","changes":[]}]),
        json!(vec![register; 65]),
    ] {
        f.request = json!({"operation":"sequence","changes":changes});
        f.refuse("propose", "invalid bounded history sequence");
    }
}

#[test]
fn sequence_command_one_step_matches_legacy_record_without_changing_legacy_request() {
    let mut f = CommandFixture::new();
    let legacy_request = f.request.clone();
    let legacy = f.ok("check");
    f.request = json!({"operation":"sequence","changes":[legacy_request.clone()]});
    let sequence = f.ok("check");
    assert_eq!(sequence["record"], legacy["record"]);
    let mut old_metadata = legacy["metadata"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["sha256"].as_str().unwrap())
        .collect::<Vec<_>>();
    let mut new_metadata = sequence["metadata"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["sha256"].as_str().unwrap())
        .collect::<Vec<_>>();
    old_metadata.sort();
    new_metadata.sort();
    assert_eq!(old_metadata, new_metadata);
    f.request = legacy_request;
    assert_eq!(f.ok("check"), legacy);
}
impl CommandFixture {
    fn new() -> Self {
        let (source, prepared, document) = new_update();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("published");
        LocalStore::create(&root, key("synthetic-place")).unwrap();
        let profile = json!({"schema":"tig.history-command.experimental-1",
            "git_executable":git_executable(),"repository":source.root(),"base":head(source.root()).as_str(),
            "records":["records"],"definitions":["definitions"],"asset":"item/1","expected":prepared.expected(),"dialect":"2020-12",
            "byte_source":{"places":[{"root":root,"layout":"flat"}],"required":if cfg!(windows) {"prevent-mutation"}else{"cooperative-immutable"}},
            "publication":{"root":root,"operation":"register-1","owner":"private-owner","required":"process_restart"},
            "proposal":{"path":"records/item.asset","identity":{"name":"Fixture","email":"fixture@example.invalid","unix_seconds":1700000000},"message":"private message"}});
        let request = json!({"operation":"register-revision","document":{"path":"document.json","expected":Object::from_bytes(&document)},"coverage":"complete"});
        Self {
            source,
            temp,
            root,
            profile,
            request,
            document,
        }
    }
    fn run(&self, action: &str) -> std::process::Output {
        fs::write(
            self.temp.path().join("profile.json"),
            serde_json::to_vec(&self.profile).unwrap(),
        )
        .unwrap();
        fs::write(
            self.temp.path().join("request.json"),
            serde_json::to_vec(&self.request).unwrap(),
        )
        .unwrap();
        fs::write(self.temp.path().join("document.json"), &self.document).unwrap();
        Command::new(env!("CARGO_BIN_EXE_git-asset-history"))
            .arg(action)
            .arg(self.temp.path().join("profile.json"))
            .arg(self.temp.path().join("request.json"))
            .output()
            .unwrap()
    }
    fn ok(&self, action: &str) -> Value {
        let output = self.run(action);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        for secret in [
            "PRIVATE AUTHORING TEXT",
            "private-owner",
            "private message",
            "item/1",
        ] {
            assert!(!text.contains(secret));
        }
        assert!(!text.contains(&self.root.to_string_lossy().to_string()));
        let result: Value = serde_json::from_str(&text).unwrap();
        for flag in [
            "current_selection",
            "approval",
            "old_protection_released",
            "independent_custody_verified",
            "complete_semantic_coverage",
        ] {
            assert_eq!(result[flag], false);
        }
        result
    }
    fn refuse(&self, action: &str, reason: &str) {
        let before = self.state();
        let output = self.run(action);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains(reason), "{error}");
        assert!(!error.contains(&self.root.to_string_lossy().to_string()));
        assert_eq!(self.state(), before);
    }
    fn state(&self) -> (Vec<u8>, Vec<u8>, usize, usize) {
        (
            git(self.source.root(), &["rev-parse", "HEAD"]).stdout,
            git(self.source.root(), &["for-each-ref"]).stdout,
            operation_files(&self.root),
            fs::read_dir(self.root.join("objects")).unwrap().count(),
        )
    }
    fn apply(&mut self, result: &Value) {
        git(
            self.source.root(),
            &[
                "cherry-pick",
                result["proposal"]["commit"].as_str().unwrap(),
            ],
        );
        self.profile["base"] = json!(head(self.source.root()).as_str());
        self.profile["expected"] = result["record"].clone();
    }
    fn document(&mut self, operation: &str, value: Value) {
        self.document = serde_json::to_vec(&value).unwrap();
        self.request = json!({"operation":operation,"document":{"path":"document.json","expected":Object::from_bytes(&self.document)}});
        if operation != "configure-delivery" {
            self.request["coverage"] = json!("complete");
        }
    }
}

#[test]
fn command_check_publish_propose_retry_and_noop_are_separate_and_sparse() {
    let mut f = CommandFixture::new();
    let before = f.state();
    let original = fs::read(f.source.root().join("records/item.asset")).unwrap();
    let check = f.ok("check");
    assert_eq!(check["published"], false);
    assert_eq!(check["metadata"].as_array().unwrap().len(), 2);
    assert_eq!(f.state(), before);
    let published = f.ok("publish");
    assert_eq!(published["published"], true);
    assert!(published["proposal"].is_null());
    assert_eq!(
        fs::read(f.source.root().join("records/item.asset")).unwrap(),
        original
    );
    let proposal = f.ok("propose");
    let retry = f.ok("propose");
    assert_eq!(proposal["proposal"], retry["proposal"]);
    assert_eq!(proposal["record"], check["record"]);
    assert_eq!(
        git(f.source.root(), &["rev-parse", "HEAD"]).stdout,
        before.0
    );
    f.apply(&proposal);
    let before = f.state();
    let noop = f.ok("propose");
    assert_eq!(noop["noop"], true);
    assert_eq!(noop["published"], false);
    assert!(noop["proposal"].is_null());
    assert_eq!(f.state(), before);
}

#[test]
fn command_select_configure_and_record_attempt_keep_the_same_asset_and_status() {
    let mut f = CommandFixture::new();
    let revision = Object::from_bytes(&f.document);
    let first = f.ok("propose");
    f.apply(&first);
    f.profile["publication"]["operation"] = json!("select-1");
    f.request = json!({"operation":"select-revision","revision":revision,"representation":"main",
        "subject":Object::from_bytes(b"not copied media"),"status":{"editorial":"draft"},"streams":[]});
    let selected = f.ok("propose");
    assert!(selected["metadata"].as_array().unwrap().is_empty());
    f.apply(&selected);
    let selected_value: Value =
        serde_json::from_slice(&fs::read(f.source.root().join("records/item.asset")).unwrap())
            .unwrap();
    assert_eq!(selected_value["status"]["editorial"], "draft");
    let target = Object::from_bytes(
        &identity::canonical_json(
            &json!({"schema":"tig.declared-publication-target.experimental-1",
        "provider":"synthetic","scope":"private-scope","path":"private-target"}),
        )
        .unwrap(),
    )
    .sha256;
    let profile = Object::from_bytes(b"opaque rendition profile");
    let policy = json!({"schema":"tig.delivery-policy.experimental-1","targets":[{"key":"drive","identity":target,
        "destination":Object::from_bytes(b"opaque destination"),"representation":"main","profile":profile,"gates":Object::from_bytes(b"opaque gates")} ]});
    let policy_pin = Object::from_bytes(&serde_json::to_vec(&policy).unwrap());
    f.document("configure-delivery", policy);
    f.profile["publication"]["operation"] = json!("policy-1");
    let configured = f.ok("propose");
    f.apply(&configured);
    f.document("record-delivery",json!({"schema":"tig.delivery-event.experimental-1","asset":"item/1","event":"attempt-1","operation":"remote-attempt-1",
        "parents":[],"policy":policy_pin,"target":"drive","revision":revision,"representation":"main","subject":Object::from_bytes(b"not copied media"),
        "profile":profile,"stage":"attempted"}));
    f.profile["publication"]["operation"] = json!("delivery-1");
    let event = f.ok("propose");
    f.apply(&event);
    let repository = reader(f.source.root());
    let snapshot = capture(&repository, head(f.source.root()));
    let history = snapshot
        .inspect_history(
            "item/1",
            Dialect::Draft202012,
            revisions::Limits::default(),
            &mut read_cache(&f.root),
        )
        .unwrap();
    assert_eq!(history.summary().delivery_events, 1);
    assert_eq!(history.summary().targets.claims, 0);
    assert!(!history.summary().current_remote_content_verified);
    let final_value: Value =
        serde_json::from_slice(&fs::read(f.source.root().join("records/item.asset")).unwrap())
            .unwrap();
    assert_eq!(final_value["id"], selected_value["id"]);
    assert_eq!(final_value["selected"], selected_value["selected"]);
    assert_eq!(final_value["status"], selected_value["status"]);
    assert_eq!(final_value["authored"], selected_value["authored"]);
}

#[test]
fn command_refuses_bad_inputs_before_publishing_or_creating_refs() {
    let mut f = CommandFixture::new();
    f.refuse("unknown", "usage:");
    f.profile["extra-private-field"] = json!(true);
    f.refuse("propose", "invalid input fields");
    f.profile
        .as_object_mut()
        .unwrap()
        .remove("extra-private-field");
    let expected = f.profile["expected"].clone();
    f.profile["expected"] = json!(Object::from_bytes(b"stale"));
    f.refuse("propose", "Stale");
    f.profile["expected"] = expected;
    f.request["document"]["expected"] = json!(Object::from_bytes(b"wrong document"));
    f.refuse("publish", "document pin differs");
    f.request["document"]["expected"] = json!(Object::from_bytes(&f.document));
    f.profile["proposal"]["path"] = json!("records/absent.asset");
    f.refuse("propose", "Missing");
    f.profile["proposal"]["path"] = json!("elsewhere/item.asset");
    f.refuse("propose", "outside declared record scope");
    f.profile["proposal"]["path"] = json!("records/item.asset");
    f.profile["publication"]["required"] = json!("provider_durable");
    f.refuse("propose", "Unsupported");
    f.profile["publication"]["required"] = json!("process_restart");
    f.profile["publication"]["root"] = json!(f.temp.path().join("never-created"));
    f.refuse("publish", "explicit reader place");
    assert!(!f.temp.path().join("never-created").exists());
    f.profile["publication"]["root"] = json!(f.root);
    f.document.push(b' ');
    f.refuse("propose", "document pin differs");
    f.request["document"]["expected"] = json!(Object::from_bytes(&f.document));
    let request = f.request.clone();
    f.request["oversized"] = json!("x".repeat(65_536));
    f.refuse("publish", "exceeds budget");
    f.request = request;
    f.request["unknown"] = json!(true);
    f.refuse("propose", "invalid input fields");
}

#[test]
fn command_missing_or_corrupt_history_never_resets_or_publishes_new_history() {
    let mut f = CommandFixture::new();
    let first = f.ok("propose");
    f.apply(&first);
    let record: Value =
        serde_json::from_slice(&fs::read(f.source.root().join("records/item.asset")).unwrap())
            .unwrap();
    let pin: Object = serde_json::from_value(
        record["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["role"] == "history/revisions")
            .unwrap()["object"]
            .clone(),
    )
    .unwrap();
    let path = Layout::Flat.object_path(&f.root, &pin.sha256);
    let quarantine = f.temp.path().join("retained-history");
    fs::rename(&path, &quarantine).unwrap();
    f.refuse("propose", "Missing");
    fs::rename(&quarantine, &path).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        f.refuse("publish", "Busy");
        drop(held);
    }
    #[cfg(not(windows))]
    {
        f.profile["byte_source"]["required"] = json!("prevent-mutation");
        f.refuse("publish", "Unsupported");
        f.profile["byte_source"]["required"] = json!("cooperative-immutable");
    }
    fs::write(&path, b"corrupt history").unwrap();
    f.refuse("publish", "Pin");
    assert_eq!(fs::read(&path).unwrap(), b"corrupt history");
}

#[test]
fn hosted_service_admits_settings_but_never_opens_a_provider_on_check_or_preflight_failure() {
    use git_asset::{history_command, source::native::metadata_reader::CacheMetadataReader};
    let mut f = CommandFixture::new();
    f.profile["publication"]["settings"] = json!({"kind":"synthetic-existing-cache"});
    f.ok("check"); // writes only this fixture's explicit input files
    let profile = f.temp.path().join("profile.json");
    let request = f.temp.path().join("request.json");
    let forbidden = |_: &history_command::Publication| -> Result<LocalStore, publication::Error> {
        panic!("provider factory invoked before admission")
    };
    assert!(history_command::execute("check", &profile, &request, forbidden).is_ok());
    f.refuse("propose", "Unsupported"); // default binary refuses other hosts' settings
    let old = f.profile["expected"].clone();
    f.profile["expected"] = json!(Object::from_bytes(b"stale"));
    f.run("check");
    assert!(
        history_command::execute("propose", &profile, &request, forbidden)
            .unwrap_err()
            .contains("Stale")
    );
    f.profile["expected"] = old;
    f.run("check");
    let result = history_command::execute("propose", &profile, &request, |enrollment| {
        assert_eq!(
            enrollment.settings,
            json!({"kind":"synthetic-existing-cache"})
        );
        assert_eq!(enrollment.root, f.root);
        LocalStore::open(&enrollment.root).map_err(|_| publication::Error::Provider)
    })
    .unwrap();
    assert_eq!(result["new_metadata_readback_verified"], true);
    let object: Object = serde_json::from_value(result["record"].clone()).unwrap();
    let mut reader = CacheMetadataReader::new(Some(
        serde_json::from_value(f.profile["byte_source"].clone()).unwrap(),
    ))
    .unwrap();
    assert_eq!(
        Object::from_bytes(&reader.read(&object, usize::MAX).unwrap()),
        object
    );
    assert_eq!(reader.read(&object, 1), Err(revisions::Error::Budget));
    let excessive = Object {
        sha256: object.sha256.clone(),
        bytes: 16 * 1024 * 1024 + 1,
    };
    assert_eq!(
        reader.read(&excessive, usize::MAX),
        Err(revisions::Error::Budget)
    );
    f.apply(&result);
    f.run("check");
    let noop = history_command::execute("propose", &profile, &request, forbidden).unwrap();
    assert_eq!(noop["noop"], true);
    assert!(noop["proposal"].is_null());
}

#[test]
fn hosted_publication_with_wrong_reader_place_keeps_bytes_and_refuses_git_until_corrected_retry() {
    use git_asset::history_command;
    let mut f = CommandFixture::new();
    f.ok("check");
    let profile = f.temp.path().join("profile.json");
    let request = f.temp.path().join("request.json");
    let actual = f.temp.path().join("actual-provider");
    LocalStore::create(&actual, key("actual-provider")).unwrap();
    let before = f.state();
    let error = history_command::execute("propose", &profile, &request, |_| {
        LocalStore::open(&actual).map_err(|_| publication::Error::Provider)
    })
    .unwrap_err();
    assert!(error.contains("readback: Missing"), "{error}");
    assert!(!error.contains(&actual.to_string_lossy().to_string()));
    assert_eq!(f.state(), before);
    let retained = operation_files(&actual);
    assert!(retained > 0); // honest partial workflow: publication succeeded, adoption did not
    f.profile["publication"]["root"] = json!(actual);
    f.profile["byte_source"]["places"][0]["root"] = json!(actual);
    f.ok("check");
    let result = history_command::execute("propose", &profile, &request, |enrollment| {
        LocalStore::open(&enrollment.root).map_err(|_| publication::Error::Provider)
    })
    .unwrap();
    assert_eq!(operation_files(&actual), retained);
    assert_eq!(result["new_metadata_readback_verified"], true);
    assert!(!result["proposal"].is_null());
    assert_eq!(
        git(f.source.root(), &["rev-parse", "HEAD"]).stdout,
        before.0
    );
}
