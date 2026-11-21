mod common;
use common::{Fixture, command, git};
use git_asset::pointer::Pointer;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn source(f: &Fixture) -> (PathBuf, String) {
    let repo = f.temp.path().join("source");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(
        repo.join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    fs::write(
        repo.join("payload.bin"),
        Pointer::new(f.objects[0].clone()).to_bytes(),
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "first"]);
    let first = String::from_utf8(git(&repo, &["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_owned();
    fs::write(
        repo.join("payload.bin"),
        Pointer::new(f.objects[1].clone()).to_bytes(),
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "second"]);
    (repo, first)
}

fn consumer(f: &Fixture, source: &Path, name: &str) -> PathBuf {
    let dest = f.temp.path().join(name);
    git(
        f.temp.path(),
        &[
            "clone",
            "--no-checkout",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ],
    );
    git(&dest, &["lfs", "install", "--local", "--skip-smudge"]);
    let profile = f.profile_file("download profile.json");
    // Git LFS parses args with shell-style quoting on both supported platforms.
    let quoted = format!(
        "--profile '{}'",
        profile
            .to_str()
            .unwrap()
            .replace('\\', "/")
            .replace('\'', "'\\''")
    );
    for (key, value) in [
        ("lfs.standalonetransferagent", "git-asset"),
        (
            "lfs.customtransfer.git-asset.path",
            env!("CARGO_BIN_EXE_git-asset-lfs"),
        ),
        ("lfs.customtransfer.git-asset.args", quoted.as_str()),
        ("lfs.customtransfer.git-asset.direction", "download"),
        ("lfs.customtransfer.git-asset.concurrent", "true"),
        ("lfs.concurrenttransfers", "2"),
    ] {
        git(&dest, &["config", key, value]);
    }
    git(&dest, &["checkout", "main"]);
    dest
}

#[test]
fn real_git_lfs_fetch_checkout_and_previous_revision_preserve_cache() {
    let f = Fixture::new(&[b"first image", b"second image"]);
    let (src, first) = source(&f);
    let repo = consumer(&f, &src, "consumer");
    assert!(Pointer::parse(&fs::read(repo.join("payload.bin")).unwrap()).is_ok());
    git(&repo, &["lfs", "fetch", "origin", "main"]);
    git(&repo, &["lfs", "checkout"]);
    assert_eq!(fs::read(repo.join("payload.bin")).unwrap(), b"second image");
    git(&repo, &["checkout", &first]);
    git(&repo, &["lfs", "fetch", "origin", &first]);
    git(&repo, &["lfs", "checkout"]);
    assert_eq!(fs::read(repo.join("payload.bin")).unwrap(), b"first image");
    assert_eq!(fs::read(f.object_path(0)).unwrap(), b"first image");
    assert_eq!(fs::read(f.object_path(1)).unwrap(), b"second image");
    assert!(f.aliases().is_empty());
    let output = git(&repo, &["lfs", "pointer", "--file=payload.bin"]);
    assert_eq!(output.stdout, Pointer::new(f.objects[0].clone()).to_bytes());
}

#[test]
fn separate_git_lfs_clients_share_admission_without_a_global_lock() {
    let f = Fixture::new(&[b"first image", b"second image"]);
    let (src, _) = source(&f);
    let a = consumer(&f, &src, "consumer a");
    let b = consumer(&f, &src, "consumer b");
    let one = command(&a, &["lfs", "fetch", "origin", "main"])
        .spawn()
        .unwrap();
    let two = command(&b, &["lfs", "fetch", "origin", "main"])
        .spawn()
        .unwrap();
    let one = one.wait_with_output().unwrap();
    let two = two.wait_with_output().unwrap();
    assert!(one.status.success(), "first concurrent LFS client failed");
    assert!(two.status.success(), "second concurrent LFS client failed");
    for repo in [&a, &b] {
        git(repo, &["lfs", "checkout"]);
        assert_eq!(fs::read(repo.join("payload.bin")).unwrap(), b"second image");
    }
    assert!(f.aliases().is_empty());
    assert_eq!(fs::read(f.object_path(1)).unwrap(), b"second image");
}

#[test]
fn real_git_lfs_rejects_corruption_and_can_retry_after_repair() {
    let f = Fixture::new(&[b"first image", b"second image"]);
    let (src, _) = source(&f);
    let repo = consumer(&f, &src, "consumer");
    fs::write(f.object_path(1), b"broken image").unwrap();
    assert!(
        !command(&repo, &["lfs", "fetch", "origin", "main"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(f.aliases().is_empty());
    fs::write(f.object_path(1), b"second image").unwrap();
    git(&repo, &["lfs", "fetch", "origin", "main"]);
    git(&repo, &["lfs", "checkout"]);
    assert_eq!(fs::read(repo.join("payload.bin")).unwrap(), b"second image");
    assert_eq!(fs::read(f.object_path(1)).unwrap(), b"second image");
}

#[test]
fn exact_git_selection_controls_real_lfs_authorization_and_historical_retry() {
    use git_asset::source::{GitId, Limits, Repository, Scope};
    use git_asset_core::{
        LogicalPath,
        tree::{MemoryNodes, TreeLimits},
    };
    let mut f = Fixture::new(&[b"first image", b"second image"]);
    let (src, first) = source(&f);
    let head = String::from_utf8(git(&src, &["rev-parse", "HEAD"]).stdout).unwrap();
    let reader = Repository::new(common::git_executable(), src.clone(), Limits::default()).unwrap();
    let select = |revision: &str, fixture: &mut Fixture| {
        let snapshot = reader
            .commit(GitId::parse(revision.trim()).unwrap())
            .unwrap();
        let scope = Scope::new([LogicalPath::parse("payload.bin").unwrap()]).unwrap();
        let selected = reader.extract(&snapshot, scope).unwrap();
        let mut nodes = MemoryNodes::default();
        let root = selected
            .write_authority_tree(&mut nodes, TreeLimits::default())
            .unwrap();
        for (id, bytes) in nodes.objects() {
            fs::write(fixture.profile.authority_nodes.join(id.as_str()), bytes).unwrap();
        }
        fixture.profile.authority_root = root.root().clone();
        fixture.profile_file("download profile.json");
    };
    select(&head, &mut f);
    let repo = consumer(&f, &src, "consumer");
    git(&repo, &["lfs", "fetch", "origin", "main"]);
    git(&repo, &["lfs", "checkout"]);
    assert_eq!(fs::read(repo.join("payload.bin")).unwrap(), b"second image");
    git(&repo, &["checkout", &first]);
    // Mere presence in the cache does not authorize bytes outside this selection.
    assert!(
        !command(&repo, &["lfs", "fetch", "origin", &first])
            .output()
            .unwrap()
            .status
            .success()
    );
    select(&first, &mut f);
    git(&repo, &["lfs", "fetch", "origin", &first]);
    git(&repo, &["lfs", "checkout"]);
    assert_eq!(fs::read(repo.join("payload.bin")).unwrap(), b"first image");
    assert_eq!(fs::read(f.object_path(0)).unwrap(), b"first image");
    assert_eq!(fs::read(f.object_path(1)).unwrap(), b"second image");
    assert!(f.aliases().is_empty());
}

#[test]
fn persistent_metadata_and_source_claims_feed_real_lfs_without_loose_node_files() {
    use git_asset::source::{GitId, Limits, Repository, Scope};
    use git_asset_core::{LogicalPath, tree::TreeLimits};
    use git_asset_store::{
        metadata::{Limits as MetadataLimits, MetadataStore},
        source_claims::{Command, Selection, SourceJournal},
    };
    let mut f = Fixture::new(&[b"first image", b"second image"]);
    let (src, _) = source(&f);
    let id = String::from_utf8(git(&src, &["rev-parse", "HEAD"]).stdout).unwrap();
    let r = Repository::new(common::git_executable(), src.clone(), Limits::default()).unwrap();
    let selected = r
        .extract(
            &r.commit(GitId::parse(id.trim()).unwrap()).unwrap(),
            Scope::new([LogicalPath::parse("payload.bin").unwrap()]).unwrap(),
        )
        .unwrap();
    let root = f.temp.path().join("metadata store");
    let mut store = MetadataStore::create(
        &root,
        MetadataLimits {
            nodes: 100,
            bytes: 1024 * 1024,
            database_bytes: 4 * 1024 * 1024,
            min_free_bytes: 65_536,
        },
    )
    .unwrap();
    let tree = selected
        .write_authority_tree(&mut store, TreeLimits::default())
        .unwrap();
    f.profile.authority_nodes = root;
    f.profile.authority_root = tree.root().clone();
    f.profile.authority_checkpoint = Some(store.checkpoint());
    let repo = consumer(&f, &src, "consumer");
    assert!(
        !command(&repo, &["lfs", "fetch", "origin", "main"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(f.aliases().is_empty());
    let witness = store.finish().unwrap();
    drop(store);
    f.profile.authority_checkpoint = Some(witness);
    f.profile_file("download profile.json");
    let journal_root = f.temp.path().join("source claims");
    let mut journal =
        SourceJournal::create(&journal_root, common::key("project"), common::key("epoch")).unwrap();
    let prepared = journal
        .apply(
            common::key("prepare"),
            Command::Prepare {
                source: common::key("main"),
                owner: common::key("owner"),
                expected_revision: 0,
                selection: Selection {
                    root: tree.root().clone(),
                    context: selected.selection_digest().unwrap(),
                },
            },
        )
        .unwrap();
    git(&repo, &["lfs", "fetch", "origin", "main"]);
    git(&repo, &["lfs", "checkout"]);
    journal
        .apply(
            common::key("adopt"),
            Command::Adopt {
                source: common::key("main"),
                owner: common::key("owner"),
                expected_revision: 1,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    let minimum = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&journal_root, &minimum).unwrap();
    assert_eq!(
        journal
            .source(&common::key("main"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection
            .root,
        *tree.root()
    );
    assert_eq!(fs::read(repo.join("payload.bin")).unwrap(), b"second image");
    assert_eq!(fs::read(f.object_path(1)).unwrap(), b"second image");
    assert!(f.aliases().is_empty());
}

#[test]
fn verified_cooperative_reader_and_real_lfs_alias_handoff_share_the_same_object() {
    use git_asset_store::existing::{
        ByteSource, Limits, Place as ReadPlace, Profile as ReadProfile, ReadCache,
        RequiredProtection,
    };
    use std::io::Read;
    let f = Fixture::new(&[b"first image", b"second image"]);
    let (src, _) = source(&f);
    let repo = consumer(&f, &src, "reader consumer");
    let mut cache = ReadCache::new(ReadProfile {
        places: f
            .profile
            .places
            .iter()
            .map(|p| ReadPlace {
                root: p.root.clone(),
                layout: p.layout.clone(),
            })
            .collect(),
        required: RequiredProtection::CooperativeImmutable,
        limits: Limits::default(),
    })
    .unwrap();
    let mut held = cache.open_verified(&f.objects[1]).unwrap();
    git(&repo, &["lfs", "fetch", "origin", "main"]);
    git(&repo, &["lfs", "checkout"]);
    let mut actual = Vec::new();
    held.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, b"second image");
    assert_eq!(fs::read(repo.join("payload.bin")).unwrap(), b"second image");
    assert!(f.aliases().is_empty());
}
