//! Explicitly admitted synthetic native tools only. No existing cache enrollment.
#![cfg(windows)]
use git_asset::{
    annex::{self, managed},
    source::{Limits as GitLimits, Repository},
};
use git_asset_core::{
    Digest, LogicalPath, Object,
    tree::{Tree, TreeLimits},
};
use git_asset_store::{
    Key, existing,
    metadata::{self, MetadataReader, MetadataStore},
    protection::Limits,
    source_claims::{
        Command, Kind, Places, Selection,
        admission::{Namespace, Policy, Select},
    },
    store_protection::{Binding, Member},
};
use reel_workflow::{Artifact, ErrorKind, input::InputReader};
use reel_workflow_git_asset::{
    GuardedInputs, Preview, Snapshot,
    claims::{Handle, State},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
    process::{Command as Process, Stdio},
    time::{Duration, Instant},
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn limits() -> Limits {
    Limits {
        millis: 60_000,
        roots: 64,
        bindings: 512,
        associations: 4096,
        reads: 4096,
        read_bytes: 4 * 1024 * 1024,
    }
}
fn run(p: &annex::Profile, annex: bool, args: &[&str]) -> Vec<u8> {
    // Immutable explicit tool paths and owned synthetic roots; no shell/profile.
    let out = tempfile::tempfile().unwrap();
    let err = tempfile::tempfile().unwrap();
    let mut c = Process::new(if annex { &p.annex } else { &p.git });
    c.args(args)
        .current_dir(&p.repository)
        .env_clear()
        .env("PATH", std::env::join_paths(&p.tools).unwrap())
        .env("TEMP", &p.temporary)
        .env("TMP", &p.temporary)
        .env("USERPROFILE", &p.temporary)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &p.empty_config)
        .env("GIT_CONFIG_COUNT", "4")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_0", &p.empty_hooks)
        .env("GIT_CONFIG_KEY_1", "core.fsmonitor")
        .env("GIT_CONFIG_VALUE_1", "false")
        .env("GIT_CONFIG_KEY_2", "credential.helper")
        .env("GIT_CONFIG_VALUE_2", "")
        .env("GIT_CONFIG_KEY_3", "annex.alwayscommit")
        .env("GIT_CONFIG_VALUE_3", "false")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "file")
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(err.try_clone().unwrap());
    if let Some(root) = std::env::var_os("SystemRoot") {
        c.env("SystemRoot", root);
    }
    let mut child = c.spawn().unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned native fixture command timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    fn bytes(mut f: fs::File) -> Vec<u8> {
        use std::io::{Seek, SeekFrom};
        f.seek(SeekFrom::Start(0)).unwrap();
        let mut b = vec![];
        f.take(16385).read_to_end(&mut b).unwrap();
        assert!(b.len() <= 16384);
        b
    }
    let output = bytes(out);
    let errors = bytes(err);
    assert!(
        status.success(),
        "synthetic command failed: {}",
        String::from_utf8_lossy(&errors)
    );
    output
}
fn text(b: Vec<u8>) -> String {
    String::from_utf8(b).unwrap().trim().into()
}
fn write(root: &Path, name: &str, value: serde_json::Value) -> Object {
    let b = serde_json::to_vec(&value).unwrap();
    fs::write(root.join(name), &b).unwrap();
    Object::from_bytes(&b)
}
struct Fixture {
    temp: tempfile::TempDir,
    profile: managed::Profile,
    source: Snapshot,
    preview: Preview,
    artifact: Artifact,
    nodes: MetadataReader,
    handle: Handle,
    shadow: Namespace,
    wrong_policy: Handle,
    missing_source: Snapshot,
    missing_preview: Preview,
    missing_artifact: Artifact,
}
impl Fixture {
    fn new(bare: bool) -> Self {
        Self::new_sized(bare, 256 * 1024)
    }
    fn new_sized(bare: bool, size: usize) -> Self {
        let temp = tempfile::Builder::new()
            .prefix("reel-public-native-fixture-")
            .tempdir()
            .unwrap();
        let root = temp.path();
        for name in ["pool", "project", "hooks", "temporary"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        fs::write(root.join("empty-config"), b"").unwrap();
        let mut pool = annex::Profile {
            git: std::env::var_os("GIT_ASSET_TEST_GIT_EXE")
                .expect("explicit Git")
                .into(),
            annex: std::env::var_os("GIT_ASSET_TEST_ANNEX_EXE")
                .expect("explicit annex")
                .into(),
            repository: root.join("pool"),
            bare,
            uuid: "00000000-0000-0000-0000-000000000001".into(),
            configuration: Object::from_bytes(b"uninitialized synthetic profile"),
            tools: std::env::split_paths(
                &std::env::var_os("GIT_ASSET_TEST_ANNEX_PATH")
                    .expect("explicit native runtime PATH"),
            )
            .collect(),
            empty_config: root.join("empty-config"),
            empty_hooks: root.join("hooks"),
            temporary: root.join("temporary"),
            command_millis: 60000,
            output_bytes: 16384,
            limits: existing::Limits {
                object_bytes: (size as u64).max(1024 * 1024),
                total_bytes: (size as u64 * 4).max(16 * 1024 * 1024),
                requests: 32,
                live_readers: 8,
                verify_millis: 30000,
            },
        };
        run(
            &pool,
            false,
            if bare {
                &["init", "--bare", "--quiet"]
            } else {
                &["init", "--quiet"]
            },
        );
        run(&pool, false, &["config", "user.name", "Synthetic Fixture"]);
        run(
            &pool,
            false,
            &["config", "user.email", "fixture@example.invalid"],
        );
        run(&pool, true, &["init", "synthetic-public-input-claim"]);
        let bytes = vec![37u8; size];
        let object = Object::from_bytes(&bytes);
        let input = root.join("synthetic-input.bin");
        fs::write(&input, &bytes).unwrap();
        run(
            &pool,
            true,
            &[
                "setkey",
                &format!("SHA256-s{}--{}", object.bytes, object.sha256.as_str()),
                input.to_str().unwrap(),
            ],
        );
        pool.uuid = text(run(
            &pool,
            false,
            &["config", "--local", "--get", "annex.uuid"],
        ));
        let mut namespaces = Vec::new();
        let mut locations = Vec::new();
        for index in 0..2 {
            let classes = [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .map(|kind| {
                    let working = if index == 1 && kind == Kind::Job {
                        "elsewhere"
                    } else {
                        "pool"
                    };
                    (
                        kind,
                        Places {
                            current: vec![key(working)],
                            pending: vec![key("pool")],
                            predecessor: vec![key("pool")],
                        },
                    )
                })
                .collect();
            let path = root.join(format!("namespace-{index}"));
            let mut ns = Namespace::create_fenced(
                &path,
                key("fixture"),
                key("epoch"),
                Policy {
                    prefer_active: true,
                    classes,
                },
            )
            .unwrap();
            // Enrollment is deliberate fixture setup, never adapter resume behavior.
            let mut empty = MetadataStore::create(
                &root.join(format!("bootstrap-{index}")),
                metadata::Limits {
                    nodes: 1024,
                    bytes: 1024 * 1024,
                    database_bytes: 4 * 1024 * 1024,
                    min_free_bytes: 65536,
                },
            )
            .unwrap();
            let cp = empty.finish().unwrap();
            drop(empty);
            let nodes =
                MetadataReader::open(&root.join(format!("bootstrap-{index}")), &cp).unwrap();
            ns.apply(
                key("enroll-attempt"),
                Command::Enroll {
                    source: key("attempt"),
                    owner: key("owner"),
                    class: Kind::Job,
                },
                &nodes,
                &limits(),
            )
            .unwrap();
            locations.push(managed::Member {
                root: path,
                minimum: ns.checkpoint(),
            });
            namespaces.push(ns);
        }
        let binding = Binding::new(
            1,
            Digest::blob(b"synthetic participating native actor boundary"),
            namespaces
                .iter()
                .map(|n| Member {
                    namespace: n.identity().clone(),
                    places: vec![key("pool")],
                })
                .collect(),
        )
        .unwrap();
        run(
            &pool,
            false,
            &[
                "config",
                "--local",
                "tig.protectionbinding",
                binding.id().unwrap().as_str(),
            ],
        );
        pool.configuration = Object::from_bytes(
            &fs::read(
                pool.repository
                    .join(if bare { "config" } else { ".git/config" }),
            )
            .unwrap(),
        );
        let profile = managed::Profile {
            pool: pool.clone(),
            binding,
            members: locations,
        };
        let mut project = pool.clone();
        project.repository = root.join("project");
        project.bare = false;
        run(&project, false, &["init", "--quiet"]);
        run(&project, false, &["config", "user.name", "Fixture"]);
        run(
            &project,
            false,
            &["config", "user.email", "fixture@example.invalid"],
        );
        run(&project, false, &["config", "commit.gpgsign", "false"]);
        for d in ["records", "definitions"] {
            fs::create_dir(project.repository.join(d)).unwrap();
        }
        let schema = write(
            &project.repository,
            "definitions/schema.json",
            json!({"type":"object"}),
        );
        let definition = write(
            &project.repository,
            "definitions/type.json",
            json!({"schema":"tig.definition.design-1","id":"types/binary","authored_schema":schema,
            "authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        let record = |payload: &Object| {
            json!({"schema":"tig.instance.design-1","id":"binary/item","definition":{"id":"types/binary","sha256":definition.sha256,"bytes":definition.bytes},
            "authored":{},"streams":[],"selected":payload,"status":{},"metadata":{}})
        };
        write(&project.repository, "records/item.asset", record(&object));
        run(&project, false, &["add", "."]);
        run(
            &project,
            false,
            &["commit", "--quiet", "-m", "original synthetic asset"],
        );
        let old = text(run(&project, false, &["rev-parse", "HEAD"]));
        write(
            &project.repository,
            "records/item.asset",
            record(&Object::from_bytes(
                b"new selection absent from native pool",
            )),
        );
        run(&project, false, &["add", "."]);
        run(
            &project,
            false,
            &["commit", "--quiet", "-m", "new synthetic selection"],
        );
        let new_id = text(run(&project, false, &["rev-parse", "HEAD"]));
        // The bare scenario also resolves metadata without a project worktree.
        let repository = if bare {
            let path = root.join("project.git");
            run(
                &project,
                false,
                &[
                    "clone",
                    "--bare",
                    "--quiet",
                    project.repository.to_str().unwrap(),
                    path.to_str().unwrap(),
                ],
            );
            path
        } else {
            project.repository.clone()
        };
        let reader = Repository::new(pool.git.clone(), repository, GitLimits::default()).unwrap();
        let source = Snapshot::capture(
            &reader,
            &old,
            &["records/item.asset".into()],
            &["definitions".into()],
        )
        .unwrap();
        let artifact = Artifact {
            logical_id: "binary/item".into(),
            sha256: object.sha256.as_str().into(),
            bytes: object.bytes,
            media_type: "application/octet-stream".into(),
        };
        let preview = source
            .preview(
                std::slice::from_ref(&artifact),
                &BTreeMap::from([(artifact.logical_id.clone(), None)]),
                (size as u64).max(1024 * 1024),
            )
            .unwrap();
        let missing_source = Snapshot::capture(
            &reader,
            &new_id,
            &["records/item.asset".into()],
            &["definitions".into()],
        )
        .unwrap();
        let missing = Object::from_bytes(b"new selection absent from native pool");
        let missing_artifact = Artifact {
            logical_id: artifact.logical_id.clone(),
            sha256: missing.sha256.as_str().into(),
            bytes: missing.bytes,
            media_type: artifact.media_type.clone(),
        };
        let missing_preview = missing_source
            .preview(
                std::slice::from_ref(&missing_artifact),
                &BTreeMap::from([(missing_artifact.logical_id.clone(), None)]),
                1024 * 1024,
            )
            .unwrap();
        let mut writer = MetadataStore::create(
            &root.join("nodes"),
            metadata::Limits {
                nodes: 10000,
                bytes: 4 * 1024 * 1024,
                database_bytes: 16 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let (tree, _) = Tree::build(
            &BTreeMap::from([(LogicalPath::parse("inputs/item").unwrap(), object)]),
            &mut writer,
            TreeLimits::default(),
        )
        .unwrap();
        let cp = writer.finish().unwrap();
        drop(writer);
        let nodes = MetadataReader::open(&root.join("nodes"), &cp).unwrap();
        let selection = Selection {
            root: tree.root().clone(),
            context: Object::from_bytes(&preview.encode().unwrap()).sha256,
        };
        let request = Select {
            prepare: key("prepare"),
            adopt: key("adopt"),
            source: key("attempt"),
            owner: key("owner"),
            expected_revision: 1,
            selection,
        };
        let wrong_policy = Handle::prepare(&mut namespaces[1], request.clone()).unwrap();
        // A stable copied identity must not redirect writes away from the pool's roster location.
        drop(namespaces.remove(0));
        let shadowroot = root.join("shadow");
        fs::create_dir(&shadowroot).unwrap();
        for file in ["claims.json", "claims.sqlite"] {
            fs::copy(root.join("namespace-0").join(file), shadowroot.join(file)).unwrap();
        }
        fs::create_dir(shadowroot.join("admission-fences")).unwrap();
        for partition in 0..256 {
            fs::write(
                shadowroot
                    .join("admission-fences")
                    .join(format!("{partition:02x}")),
                b"",
            )
            .unwrap();
        }
        let mut shadow = Namespace::open(&shadowroot, &profile.members[0].minimum).unwrap();
        let handle = Handle::prepare(&mut shadow, request).unwrap();
        Self {
            temp,
            profile,
            source,
            preview,
            artifact,
            nodes,
            handle,
            shadow,
            wrong_policy,
            missing_source,
            missing_preview,
            missing_artifact,
        }
    }
}
fn exercise(bare: bool) {
    let mut f = Fixture::new(bare);
    let mut inputs =
        GuardedInputs::new(f.profile.clone(), &f.source, &f.preview, 8 * 1024 * 1024).unwrap();
    assert_eq!(
        inputs
            .admit_claim(&f.handle, &mut f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Denied
    );
    assert_eq!(
        inputs.read(&f.artifact, f.artifact.bytes).unwrap(),
        vec![37u8; 256 * 1024]
    );
    assert_eq!(
        inputs
            .admit_claim(&f.wrong_policy, &mut f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Denied
    );
    let mut changed = f.handle.request().clone();
    changed.selection.context = Digest::blob(b"wrong preview");
    let changed = Handle::prepare(&mut f.shadow, changed).unwrap();
    assert_eq!(
        inputs
            .admit_claim(&changed, &mut f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    let cp = f.nodes.checkpoint().clone();
    let path = f.temp.path().join("nodes");
    let mut writer = MetadataStore::open(&path, &cp).unwrap();
    let (omitted, _) = Tree::build(
        &BTreeMap::from([(
            LogicalPath::parse("unrelated").unwrap(),
            Object::from_bytes(b"unrelated declared object"),
        )]),
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let cp = writer.finish().unwrap();
    drop(writer);
    f.nodes = MetadataReader::open(&path, &cp).unwrap();
    let mut request = f.handle.request().clone();
    request.selection.root = omitted.root().clone();
    let omitted = Handle::prepare(&mut f.shadow, request).unwrap();
    assert_eq!(
        inputs
            .admit_claim(&omitted, &mut f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    let mut request = f.handle.request().clone();
    request.selection.root = Object::from_bytes(b"missing flat metadata root");
    let unknown = Handle::prepare(&mut f.shadow, request).unwrap();
    assert_eq!(
        inputs
            .admit_claim(&unknown, &mut f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Missing
    );
    let mut budget = limits();
    budget.read_bytes = 1;
    assert_eq!(
        inputs
            .admit_claim(&f.handle, &mut f.nodes, &budget)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
    let mut actual = Namespace::open(&f.profile.members[0].root, f.handle.minimum()).unwrap();
    assert!(
        actual
            .source(&key("attempt"))
            .unwrap()
            .unwrap()
            .current
            .is_none()
    );
    let mut incomplete = f.profile.clone();
    incomplete.members.pop();
    assert!(GuardedInputs::new(incomplete, &f.source, &f.preview, 1024 * 1024).is_err());
    let status = inputs
        .admit_claim(&f.handle, &mut f.nodes, &limits())
        .unwrap();
    assert_eq!(status.state, State::Active);
    let protected = inputs
        .protect_claim(&f.handle, &mut f.nodes, &limits())
        .unwrap();
    // The handle came from copied metadata, but the guard must fence the real
    // configured namespace. Shared byte guards alone permit claim mutation.
    assert_eq!(
        f.handle
            .cancel(
                &mut actual,
                key("cancel-while-protected"),
                &f.nodes,
                &limits()
            )
            .unwrap_err()
            .kind,
        ErrorKind::Busy
    );
    actual
        .apply(
            key("enroll-independent-generation"),
            Command::Enroll {
                source: key("independent-generation"),
                owner: key("owner"),
                class: Kind::Retention,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    let other = Select {
        prepare: key("independent-prepare"),
        adopt: key("independent-adopt"),
        source: key("independent-generation"),
        owner: key("owner"),
        expected_revision: 1,
        selection: omitted.request().selection.clone(),
    };
    actual.select(&other, &f.nodes, &limits()).unwrap();
    assert_eq!(f.handle.status(&mut actual).unwrap().state, State::Active);

    // Compose exact Git snapshot -> native guarded bytes -> durable claim ->
    // owned streaming workspace, without another cache/provider implementation.
    use reel_workflow_git_asset::workspace::{Budget, Preparation, Ticket, Workspace};
    let preparation = Preparation::capture(
        &f.source,
        &f.preview,
        BTreeMap::from([(f.artifact.logical_id.clone(), "binary/item.bin".into())]),
        Budget {
            logical_bytes: 1024 * 1024,
            peak_file_data: 8 * 1024 * 1024,
            millis: 60000,
        },
    )
    .unwrap();
    let control = preparation.control().to_vec();
    let control_pin = Object::from_bytes(&control);
    let parent = f.temp.path().join("owned-workspaces");
    fs::create_dir(&parent).unwrap();
    let (ticket, mut workspace) = Workspace::create(&parent, "original", preparation).unwrap();
    let ticket_raw = ticket.encode().unwrap();
    let ticket_pin = Object::from_bytes(&ticket_raw);
    let mut native_source = managed::Reader::new(f.profile.clone()).unwrap();
    assert!(
        !workspace
            .materialize(&mut native_source)
            .unwrap()
            .already_complete
    );
    assert_eq!(
        fs::read(workspace.input_path(&f.artifact.logical_id).unwrap()).unwrap(),
        vec![37u8; 256 * 1024]
    );
    drop(workspace);
    drop(native_source);
    let preparation = Preparation::restore(&control, &control_pin).unwrap();
    let ticket = Ticket::decode(&ticket_raw, &ticket_pin).unwrap();
    let mut workspace = Workspace::reopen(&ticket, preparation).unwrap();
    assert!(workspace.verify().unwrap().already_complete);
    drop(workspace);

    assert!(
        f.shadow
            .source(&key("attempt"))
            .unwrap()
            .unwrap()
            .current
            .is_none()
    );
    let object = Object::from_bytes(&vec![37u8; 256 * 1024]);
    let mut current = Namespace::open(&f.profile.members[0].root, f.handle.minimum()).unwrap();
    assert!(matches!(
        current.fence(&object),
        Err(git_asset_store::source_claims::admission::Error::Busy)
    ));
    drop(protected);
    let fence = current.fence(&object).unwrap();
    assert!(
        current
            .absence(&fence, &key("pool"), &f.nodes, &limits())
            .unwrap()
            .is_none()
    );
    drop(fence);
    assert_eq!(f.handle.status(&mut current).unwrap().state, State::Active);
    assert!(inputs_missing(&f));
    let mut missing = GuardedInputs::new(
        f.profile.clone(),
        &f.missing_source,
        &f.missing_preview,
        1024 * 1024,
    )
    .unwrap();
    assert_eq!(
        missing
            .read(&f.missing_artifact, f.missing_artifact.bytes)
            .unwrap_err()
            .kind,
        ErrorKind::Missing
    );

    let _owned_root = f.temp.path(); // keeps all synthetic roots alive through assertions
}
fn inputs_missing(f: &Fixture) -> bool {
    let mut changed = f.preview.clone();
    changed.inputs[0].object = Object::from_bytes(b"not from original snapshot");
    GuardedInputs::new(f.profile.clone(), &f.source, &changed, 1024 * 1024).is_err()
}
#[test]
#[ignore = "requires explicitly admitted native git and git-annex runtime"]
fn ordinary_native_input_guards_become_original_durable_claims() {
    exercise(false)
}
#[test]
#[ignore = "requires explicitly admitted native git and git-annex runtime"]
fn bare_native_input_guards_become_original_durable_claims() {
    exercise(true)
}

#[path = "common/mod.rs"]
mod common;
#[path = "native/lifecycle.rs"]
mod lifecycle;
#[path = "native/retention.rs"]
mod retention;
#[path = "native/returns.rs"]
mod returns;

#[test]
#[ignore = "explicit native tools; fresh synthetic 257 MiB input; streaming guard only"]
fn streamed_native_input_exceeds_small_vector_read_limit() {
    let size = 257 * 1024 * 1024;
    let f = Fixture::new_sized(false, size);
    let mut inputs =
        GuardedInputs::new(f.profile.clone(), &f.source, &f.preview, size as u64).unwrap();
    assert_eq!(
        inputs.read(&f.artifact, size as u64).unwrap_err().kind,
        ErrorKind::Budget
    );
    inputs.hold_all().unwrap();
    let ns = Namespace::open(&f.profile.members[0].root, f.handle.minimum()).unwrap();
    let object = Object {
        sha256: Digest::parse(&f.artifact.sha256).unwrap(),
        bytes: f.artifact.bytes,
    };
    assert!(matches!(
        ns.fence(&object),
        Err(git_asset_store::source_claims::admission::Error::Busy)
    ));
    drop(inputs);
    drop(ns.fence(&object).unwrap());
}
