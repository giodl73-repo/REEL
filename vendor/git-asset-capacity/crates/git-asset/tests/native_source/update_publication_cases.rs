use super::*;
use git_asset::source::native::updates::publication;
use git_asset_model::revisions::Reader as _;
use git_asset_store::{
    AckLevel, Capabilities, Key, PublicationBackend, PublicationGuard, Request, State,
    existing::{ByteSource, Layout, ReadCache},
    local::LocalStore,
};
use std::io::Read;
#[path = "history_command_cases.rs"]
mod command_cases;

struct Empty;
impl revisions::Reader for Empty {
    fn read(&mut self, _: &Object, _: usize) -> Result<Vec<u8>, revisions::Error> {
        panic!("unconfigured history should not read objects")
    }
}
fn key(value: &str) -> Key {
    Key::parse(value).unwrap()
}
fn new_update() -> (Fixture, Prepared, Vec<u8>) {
    let f = Fixture::new();
    let expected = f.record("item", "item/1", true, json!([]), None);
    let base = commit(f.root());
    let repository = reader(f.root());
    let snapshot = capture(&repository, base);
    let raw = manifest(
        "item/1",
        "run-a",
        &Object::from_bytes(b"not copied media"),
        &[],
    );
    let prepared = snapshot
        .prepare_history(
            "item/1",
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            Change::RegisterRevision {
                manifest: &raw,
                coverage: Coverage::Complete,
            },
            &mut Empty,
        )
        .unwrap();
    (f, prepared, raw)
}
fn operation_files(root: &Path) -> usize {
    fs::read_dir(root.join("operations")).unwrap().count()
}
struct Reader(ReadCache);
impl revisions::Reader for Reader {
    fn read(&mut self, object: &Object, maximum: usize) -> Result<Vec<u8>, revisions::Error> {
        if object.bytes > maximum as u64 {
            return Err(revisions::Error::Budget);
        }
        let mut held = self
            .0
            .open_verified(object)
            .map_err(|_| revisions::Error::Missing)?;
        let mut raw = Vec::new();
        (&mut held)
            .take(maximum as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| revisions::Error::Missing)?;
        Ok(raw)
    }
}
fn read_cache(root: &Path) -> Reader {
    let profile = serde_json::from_value(json!({"places":[{"root":root,"layout":"flat"}],
        "required":if cfg!(windows) {"prevent-mutation"} else {"cooperative-immutable"}}))
    .unwrap();
    Reader(ReadCache::new(profile).unwrap())
}

#[test]
fn publication_reopens_in_normal_reader_and_cli_then_hands_off_one_git_record() {
    let (f, prepared, raw) = new_update();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("published");
    let mut backend = LocalStore::create(&root, key("synthetic-place")).unwrap();
    let published = publication::publish(
        &prepared,
        &key("update-1"),
        &key("owner"),
        AckLevel::ProcessRestart,
        &mut backend,
    )
    .unwrap();
    assert_eq!(published.receipts().len(), 4);
    assert!(!published.is_noop());
    assert!(!published.independent_custody_verified());
    assert!(!published.authorizes_old_protection_release());
    assert!(
        published
            .receipts()
            .iter()
            .all(|r| r.level() == AckLevel::ProcessRestart)
    );
    drop(backend);
    let mut backend = LocalStore::open(&root).unwrap();
    let before = operation_files(&root);
    let retry = publication::publish(
        &prepared,
        &key("update-1"),
        &key("owner"),
        AckLevel::ProcessRestart,
        &mut backend,
    )
    .unwrap();
    assert_eq!(retry.intent(), published.intent());
    assert_eq!(retry.receipts(), published.receipts());
    assert_eq!(operation_files(&root), before);
    for blob in prepared.metadata().iter().chain([prepared.record()]) {
        let actual = read_cache(&root).read(blob.object(), 65_536).unwrap();
        assert_eq!(actual, blob.bytes());
    }
    let repository = reader(f.root());
    let changes = [published.replacement(LogicalPath::parse("records/item.asset").unwrap())];
    let proposal = repository
        .propose(
            head(f.root()),
            &changes,
            &identity(),
            "Adopt published history",
        )
        .unwrap();
    git(f.root(), &["cherry-pick", proposal.commit().as_str()]);
    let profile = json!({"schema":"tig.asset-resolver-profile.experimental-1",
        "git_executable":git_executable(),"repository":f.root(),"commit":head(f.root()).as_str(),
        "records":["records"],"definitions":["definitions"],
        "byte_source":{"places":[{"root":root,"layout":"flat"}],"required":if cfg!(windows) {"prevent-mutation"} else {"cooperative-immutable"}}});
    let path = temp.path().join("profile.json");
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
        .arg("--history")
        .arg(path)
        .args(["item/1", "2020-12"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["revisions"], 1);
    assert_eq!(summary["payload_verified"], false);
    let snapshot = capture(&repository, head(f.root()));
    let expected = snapshot
        .resolve_id("item/1")
        .unwrap()
        .reference()
        .object
        .clone();
    let noop = snapshot
        .prepare_history(
            "item/1",
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            Change::RegisterRevision {
                manifest: &raw,
                coverage: Coverage::Complete,
            },
            &mut read_cache(&root),
        )
        .unwrap();
    assert!(noop.is_noop());
    let mut forbidden = Forbidden;
    let result = publication::publish(
        &noop,
        &key("noop"),
        &key("owner"),
        AckLevel::ProviderDurable,
        &mut forbidden,
    )
    .unwrap();
    assert!(result.is_noop());
    assert_eq!(result.record(), noop.record().object());
    assert!(result.receipts().is_empty());
}

struct Forbidden;
impl PublicationBackend for Forbidden {
    fn capabilities(&self) -> Capabilities {
        panic!("no-op touched provider")
    }
    fn publication_guard(
        &mut self,
        _: &Object,
    ) -> git_asset_store::Result<Box<dyn PublicationGuard>> {
        panic!("no-op")
    }
    fn protect(&mut self, _: &Request) -> git_asset_store::Result<State> {
        panic!("no-op")
    }
    fn verified_object(&mut self, _: &Object) -> git_asset_store::Result<bool> {
        panic!("no-op")
    }
    fn publish_object(&mut self, _: &Request, _: &mut dyn Read) -> git_asset_store::Result<()> {
        panic!("no-op")
    }
    fn commit(&mut self, _: &Request) -> git_asset_store::Result<()> {
        panic!("no-op")
    }
}

struct Fault {
    inner: LocalStore,
    commit: usize,
    fail: usize,
    after: bool,
    max_bytes: Option<u64>,
}
impl PublicationBackend for Fault {
    fn capabilities(&self) -> Capabilities {
        let mut caps = self.inner.capabilities();
        if let Some(maximum) = self.max_bytes {
            caps.max_object_bytes = maximum;
        }
        caps
    }
    fn publication_guard(
        &mut self,
        o: &Object,
    ) -> git_asset_store::Result<Box<dyn PublicationGuard>> {
        self.inner.publication_guard(o)
    }
    fn protect(&mut self, r: &Request) -> git_asset_store::Result<State> {
        self.inner.protect(r)
    }
    fn verified_object(&mut self, o: &Object) -> git_asset_store::Result<bool> {
        self.inner.verified_object(o)
    }
    fn publish_object(&mut self, r: &Request, s: &mut dyn Read) -> git_asset_store::Result<()> {
        self.inner.publish_object(r, s)
    }
    fn commit(&mut self, r: &Request) -> git_asset_store::Result<()> {
        self.commit += 1;
        if self.after {
            self.inner.commit(r)?;
        }
        if self.commit == self.fail {
            return Err(git_asset_store::Error::Io(
                std::io::ErrorKind::Other,
                "private-provider-path".into(),
            ));
        }
        if !self.after {
            self.inner.commit(r)?;
        }
        Ok(())
    }
}

#[test]
fn partial_publication_and_unknown_reply_retry_without_returning_false_completion() {
    for after in [true, false] {
        let (_f, prepared, _) = new_update();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("published");
        let inner = LocalStore::create(&root, key("synthetic-place")).unwrap();
        let mut fault = Fault {
            inner,
            commit: 0,
            fail: 2,
            after,
            max_bytes: None,
        };
        let error = publication::publish(
            &prepared,
            &key("update-1"),
            &key("owner"),
            AckLevel::ProcessRestart,
            &mut fault,
        )
        .err()
        .unwrap();
        assert_eq!(error, publication::Error::Provider);
        assert!(!format!("{error:?}").contains("private-provider-path"));
        drop(fault);
        let mut backend = LocalStore::open(&root).unwrap();
        let recovered = publication::publish(
            &prepared,
            &key("update-1"),
            &key("owner"),
            AckLevel::ProcessRestart,
            &mut backend,
        )
        .unwrap();
        assert_eq!(recovered.receipts().len(), 4);
        assert_eq!(
            read_cache(&root)
                .read(prepared.record().object(), 65_536)
                .unwrap(),
            prepared.record().bytes()
        );
    }
}

#[test]
fn a_later_oversized_object_refuses_before_any_batch_intent_is_written() {
    let (f, _, raw) = new_update();
    let mut value: Value = serde_json::from_slice(&raw).unwrap();
    value["representations"] = Value::Array((0..64).map(|i| json!({"key":format!("view-{i}"),"object":Object::from_bytes(b"unchanged payload")})).collect());
    let raw = serde_json::to_vec(&value).unwrap();
    assert!(raw.len() > 4096);
    let snapshot = capture(&reader(f.root()), head(f.root()));
    let expected = snapshot
        .resolve_id("item/1")
        .unwrap()
        .reference()
        .object
        .clone();
    let prepared = snapshot
        .prepare_history(
            "item/1",
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            Change::RegisterRevision {
                manifest: &raw,
                coverage: Coverage::Complete,
            },
            &mut Empty,
        )
        .unwrap();
    assert!(prepared.record().bytes().len() < 4096);
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("published");
    let inner = LocalStore::create(&root, key("synthetic-place")).unwrap();
    let mut backend = Fault {
        inner,
        commit: 0,
        fail: usize::MAX,
        after: false,
        max_bytes: Some(4096),
    };
    assert_eq!(
        publication::publish(
            &prepared,
            &key("large"),
            &key("owner"),
            AckLevel::ProcessRestart,
            &mut backend
        )
        .err(),
        Some(publication::Error::Budget)
    );
    assert_eq!(backend.commit, 0);
    assert_eq!(operation_files(&root), 0);
    assert_eq!(fs::read_dir(root.join("objects")).unwrap().count(), 0);
}

#[test]
fn operation_reuse_conflicts_before_children_and_unsupported_ack_has_no_intent() {
    let (f, prepared, _) = new_update();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("published");
    let mut backend = LocalStore::create(&root, key("synthetic-place")).unwrap();
    let op = key("update-1");
    let owner = key("owner");
    assert_eq!(
        publication::publish(
            &prepared,
            &op,
            &owner,
            AckLevel::ProviderDurable,
            &mut backend
        )
        .err(),
        Some(publication::Error::Unsupported)
    );
    assert_eq!(operation_files(&root), 0);
    publication::publish(
        &prepared,
        &op,
        &owner,
        AckLevel::ProcessRestart,
        &mut backend,
    )
    .unwrap();
    let before = operation_files(&root);
    assert_eq!(
        publication::publish(
            &prepared,
            &op,
            &key("other-owner"),
            AckLevel::ProcessRestart,
            &mut backend
        )
        .err(),
        Some(publication::Error::Conflict)
    );
    assert_eq!(
        publication::publish(
            &prepared,
            &op,
            &owner,
            AckLevel::VerifiedLocal,
            &mut backend
        )
        .err(),
        Some(publication::Error::Conflict)
    );
    let snapshot = capture(&reader(f.root()), head(f.root()));
    let expected = snapshot
        .resolve_id("item/1")
        .unwrap()
        .reference()
        .object
        .clone();
    let different = manifest("item/1", "run-b", &Object::from_bytes(b"B"), &[]);
    let other = snapshot
        .prepare_history(
            "item/1",
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            Change::RegisterRevision {
                manifest: &different,
                coverage: Coverage::Complete,
            },
            &mut Empty,
        )
        .unwrap();
    assert_eq!(
        publication::publish(&other, &op, &owner, AckLevel::ProcessRestart, &mut backend).err(),
        Some(publication::Error::Conflict)
    );
    assert_eq!(operation_files(&root), before);
    // The originally acknowledged descriptor is not silently repaired after corruption.
    let original = publication::publish(
        &prepared,
        &op,
        &owner,
        AckLevel::ProcessRestart,
        &mut backend,
    )
    .unwrap();
    let intent = original.intent().unwrap();
    fs::write(Layout::Flat.object_path(&root, &intent.sha256), b"corrupt").unwrap();
    assert_eq!(
        publication::publish(
            &prepared,
            &op,
            &owner,
            AckLevel::ProcessRestart,
            &mut backend
        )
        .err(),
        Some(publication::Error::Integrity)
    );
}
