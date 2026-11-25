//! Admitted-control/owned-filesystem components, not native custody simulation.
use super::*;
use crate::Input;
use git_asset_core::Digest;
#[cfg(windows)]
use std::io::Cursor;
fn body(index: u8) -> Vec<u8> {
    if index == 1 {
        vec![23; 150000]
    } else {
        b"second input".to_vec()
    }
}
fn plan(bindings: BTreeMap<String, String>) -> Result<Preparation> {
    let inputs = (1..=2)
        .map(|i| {
            let bytes = body(i);
            let object = Object::from_bytes(&bytes);
            Input {
                artifact: Artifact {
                    logical_id: format!("input/{i}"),
                    sha256: object.sha256.as_str().into(),
                    bytes: object.bytes,
                    media_type: "application/octet-stream".into(),
                },
                record: Object::from_bytes(&[i]),
                object,
                stream: None,
            }
        })
        .collect::<Vec<_>>();
    Preparation::checked(Wire {
        schema: "reel.public-input-preparation.v1".into(),
        preview: Preview {
            schema: "reel.workflow-public-git-asset-inputs.v1".into(),
            commit: "a".repeat(40),
            checkpoint: Digest::blob(b"admitted synthetic metadata"),
            logical_bytes: inputs.iter().map(|i| i.object.bytes).sum(),
            inputs,
        },
        bindings,
        budget: Budget {
            logical_bytes: 1024 * 1024,
            peak_file_data: 8 * 1024 * 1024,
            millis: 60000,
        },
    })
}
fn bindings() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("input/1".into(), "pictures/one.bin".into()),
        ("input/2".into(), "two.txt".into()),
    ])
}
fn fresh() -> Preparation {
    plan(bindings()).unwrap()
}
fn clone_plan(plan: &Preparation) -> Preparation {
    Preparation::restore(plan.control(), &Object::from_bytes(plan.control())).unwrap()
}
#[test]
fn portable_paths_case_prefix_collisions_and_exact_sets_are_checked_before_io() {
    for bad in [
        "../escape",
        "/absolute",
        "x\\escape",
        "a:b",
        "a//b",
        "con.bin",
        ".git/a",
        "trailing.",
    ] {
        let mut b = bindings();
        b.insert("input/1".into(), bad.into());
        assert_eq!(plan(b).err().unwrap().kind, ErrorKind::Invalid);
    }
    for (one, two) in [("A.bin", "a.bin"), ("a", "a/b"), ("A/one", "a/two")] {
        assert_eq!(
            plan(BTreeMap::from([
                ("input/1".into(), one.into()),
                ("input/2".into(), two.into())
            ]))
            .err()
            .unwrap()
            .kind,
            ErrorKind::Conflict
        );
    }
    assert!(plan(BTreeMap::new()).is_err());
    let p = fresh();
    let mut raw = p.control().to_vec();
    raw.push(b' ');
    assert_eq!(
        Preparation::restore(&raw, &Object::from_bytes(p.control()))
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
    assert_eq!(clone_plan(&p).control(), p.control());
}
#[test]
fn budget_refusal_precedes_owned_creation_and_control_tampering_is_rejected() {
    let p = fresh();
    let mut wire: Wire = serde_json::from_slice(p.control()).unwrap();
    wire.budget.peak_file_data = 1;
    assert_eq!(
        Preparation::checked(wire).err().unwrap().kind,
        ErrorKind::Budget
    );
    assert!(Preparation::restore(&vec![0; MAX_CONTROL + 1], &Object::from_bytes(b"x")).is_err());
    #[cfg(not(windows))]
    {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            Workspace::create(root.path(), "not-created", p)
                .err()
                .unwrap()
                .kind,
            ErrorKind::Unsupported
        );
        assert!(!root.path().join("not-created").exists());
    }
}
#[cfg(windows)]
struct Source {
    opens: usize,
    fail: bool,
    corrupt: bool,
    exit: bool,
}
#[cfg(windows)]
struct Stream {
    data: Cursor<Vec<u8>>,
    fail: bool,
    exit: bool,
}
#[cfg(windows)]
impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.exit && self.data.position() >= 65536 {
            std::process::exit(78);
        }
        if self.fail && self.data.position() >= 65536 {
            return Err(io::Error::other("injected interrupted source"));
        }
        self.data.read(buf)
    }
}
#[cfg(windows)]
impl std::io::Seek for Stream {
    fn seek(&mut self, p: std::io::SeekFrom) -> io::Result<u64> {
        self.data.seek(p)
    }
}
#[cfg(windows)]
impl ByteSource for Source {
    type Reader = Stream;
    fn open_verified(&mut self, object: &Object) -> existing::Result<Stream> {
        self.opens += 1;
        for i in 1..=2 {
            let mut bytes = body(i);
            if Object::from_bytes(&bytes) == *object {
                if self.corrupt {
                    bytes[0] ^= 1;
                }
                return Ok(Stream {
                    data: Cursor::new(bytes),
                    fail: self.fail,
                    exit: self.exit,
                });
            }
        }
        Err(existing::Error::Missing)
    }
}
#[cfg(windows)]
fn source() -> Source {
    Source {
        opens: 0,
        fail: false,
        corrupt: false,
        exit: false,
    }
}
#[test]
#[cfg(windows)]
fn owned_copy_retry_reopen_and_named_verification_do_not_scan_or_cleanup() {
    let parent = tempfile::tempdir().unwrap();
    let p = fresh();
    let restored = clone_plan(&p);
    let (ticket, mut ws) = Workspace::create(parent.path(), "owned", p).unwrap();
    let raw = ticket.encode().unwrap();
    let pin = Object::from_bytes(&raw);
    let mut input = source();
    input.fail = true;
    assert_eq!(
        ws.materialize(&mut input).unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
    let part = ws.path().join("parts/000-0.part");
    assert_eq!(fs::metadata(&part).unwrap().len(), 65536);
    let prefix = fs::read(&part).unwrap();
    input.fail = false;
    assert!(!ws.materialize(&mut input).unwrap().already_complete);
    assert_eq!(fs::read(&part).unwrap(), prefix);
    assert_eq!(input.opens, 3);
    assert!(ws.verify().unwrap().already_complete);
    let root = ws.path().to_path_buf();
    fs::write(root.join("unrelated.data"), b"leave this alone").unwrap();
    assert!(fs::rename(&root, parent.path().join("moved-while-held")).is_err());
    drop(ws);
    assert!(part.exists());
    let ticket = Ticket::decode(&raw, &pin).unwrap();
    let mut ws = Workspace::reopen(&ticket, restored).unwrap();
    let before = input.opens;
    assert!(ws.materialize(&mut input).unwrap().already_complete);
    assert_eq!(input.opens, before);
    assert_eq!(
        fs::read(ws.input_path("input/1").unwrap()).unwrap(),
        body(1)
    );
    assert_eq!(
        fs::read(root.join("unrelated.data")).unwrap(),
        b"leave this alone"
    );
}
#[test]
#[cfg(windows)]
fn scoped_workspace_exclusion_allows_other_workspaces_and_preserves_foreign_bytes() {
    let parent = tempfile::tempdir().unwrap();
    let p = fresh();
    let original = clone_plan(&p);
    let (ticket, mut ws) = Workspace::create(parent.path(), "first", p).unwrap();
    assert_eq!(
        Workspace::reopen(&ticket, original).err().unwrap().kind,
        ErrorKind::Busy
    );
    let (_other_ticket, other) = Workspace::create(parent.path(), "second", fresh()).unwrap();
    drop(other);
    let path = ws.input_path("input/1").unwrap();
    fs::write(&path, b"foreign acknowledged bytes").unwrap();
    let mut input = source();
    assert_eq!(
        ws.materialize(&mut input).unwrap_err().kind,
        ErrorKind::Integrity
    );
    assert_eq!(input.opens, 0);
    assert_eq!(fs::read(path).unwrap(), b"foreign acknowledged bytes");
}
#[test]
#[cfg(windows)]
fn exhausted_partial_slots_and_corrupt_source_refuse_without_overwrite() {
    let parent = tempfile::tempdir().unwrap();
    let (_, mut ws) = Workspace::create(parent.path(), "full", fresh()).unwrap();
    for n in 0..ATTEMPTS {
        fs::write(
            ws.path().join("parts").join(format!("000-{n}.part")),
            b"prefix",
        )
        .unwrap();
    }
    let mut input = source();
    assert_eq!(
        ws.materialize(&mut input).unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
    assert_eq!(input.opens, 0);
    let (_, mut other) = Workspace::create(parent.path(), "corrupt", fresh()).unwrap();
    input.corrupt = true;
    assert_eq!(
        other.materialize(&mut input).unwrap_err().kind,
        ErrorKind::Integrity
    );
    let path = other.path().join("parts/000-0.part");
    let before = fs::read(&path).unwrap();
    assert!(!before.is_empty());
    assert_eq!(
        other.materialize(&mut input).unwrap_err().kind,
        ErrorKind::Integrity
    );
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
#[cfg(windows)]
fn copied_owner_text_cannot_adopt_a_replaced_directory_and_leaf_kinds_refuse() {
    let parent = tempfile::tempdir().unwrap();
    let p = fresh();
    let original = clone_plan(&p);
    let (ticket, ws) = Workspace::create(parent.path(), "owned", p).unwrap();
    let root = ws.path().to_path_buf();
    drop(ws);
    // Move only the known owned fixture; never remove its contents.
    fs::rename(&root, parent.path().join("original-preserved")).unwrap();
    fs::create_dir(&root).unwrap();
    fs::copy(
        parent.path().join("original-preserved/owner.json"),
        root.join("owner.json"),
    )
    .unwrap();
    assert_eq!(
        Workspace::reopen(&ticket, original).err().unwrap().kind,
        ErrorKind::Stale
    );
    let (_, mut ws) = Workspace::create(parent.path(), "links", fresh()).unwrap();
    let outside = parent.path().join("outside.bin");
    fs::write(&outside, b"outside keep").unwrap();
    // A regular directory at a named file is always rejected, regardless of privileges.
    fs::create_dir(ws.input_path("input/1").unwrap()).unwrap();
    let mut input = source();
    assert!(ws.materialize(&mut input).is_err());
    assert_eq!(input.opens, 0);
    assert_eq!(fs::read(outside).unwrap(), b"outside keep");
}

#[test]
#[cfg(windows)]
fn reparse_parent_is_refused_before_any_source_read_or_outside_write() {
    let parent = tempfile::tempdir().unwrap();
    let p = fresh();
    let original = clone_plan(&p);
    let (ticket, ws) = Workspace::create(parent.path(), "junction", p).unwrap();
    let root = ws.path().to_path_buf();
    drop(ws);
    let directory = root.join("inputs/pictures");
    fs::rename(&directory, root.join("original-pictures")).unwrap();
    let outside = parent.path().join("outside-directory");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"preserve outside").unwrap();
    let script = parent.path().join("junction.ps1");
    fs::write(&script,"param([string]$LinkPath,[string]$TargetPath)\nNew-Item -ItemType Junction -Path $LinkPath -Target $TargetPath -ErrorAction Stop | Out-Null\n").unwrap();
    let command = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = std::process::Command::new(command)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg("-LinkPath")
        .arg(&directory)
        .arg("-TargetPath")
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "owned synthetic junction setup failed: {:?}",
        output
    );
    assert_eq!(
        Workspace::reopen(&ticket, original).err().unwrap().kind,
        ErrorKind::Denied
    );
    // Remove only the known fixture link; never recurse through its target.
    fs::remove_dir(&directory).unwrap();
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"preserve outside");
}

#[cfg(windows)]
#[derive(Serialize, Deserialize)]
struct Case {
    ticket: Vec<u8>,
    ticket_pin: Object,
    preparation: Vec<u8>,
    preparation_pin: Object,
}
#[test]
#[cfg(windows)]
fn actual_process_exit_keeps_partial_bytes_and_allows_original_ticket_retry() {
    let parent = tempfile::Builder::new()
        .prefix("reel-public-workspace-fixture-")
        .tempdir()
        .unwrap();
    let p = fresh();
    let control = p.control().to_vec();
    let (ticket, ws) = Workspace::create(parent.path(), "operation", p).unwrap();
    let root = ws.path().to_path_buf();
    drop(ws);
    let raw = ticket.encode().unwrap();
    let case = Case {
        ticket_pin: Object::from_bytes(&raw),
        ticket: raw,
        preparation_pin: Object::from_bytes(&control),
        preparation: control,
    };
    let path = parent.path().join("case.json");
    fs::write(&path, serde_json::to_vec(&case).unwrap()).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "workspace::tests::workspace_child_exit_worker",
            "--ignored",
        ])
        .env("REEL_PUBLIC_WORKSPACE_FIXTURE", &path)
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > std::time::Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned workspace child timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(78));
    let part = root.join("parts/000-0.part");
    let prefix = fs::read(&part).unwrap();
    assert_eq!(prefix.len(), 65536);
    let ticket = Ticket::decode(&case.ticket, &case.ticket_pin).unwrap();
    let preparation = Preparation::restore(&case.preparation, &case.preparation_pin).unwrap();
    let mut ws = Workspace::reopen(&ticket, preparation).unwrap();
    assert!(!ws.materialize(&mut source()).unwrap().already_complete);
    assert_eq!(fs::read(part).unwrap(), prefix);
    assert!(ws.verify().unwrap().already_complete);
}
#[test]
#[cfg(windows)]
#[ignore = "only launched by the owned synthetic workspace fixture"]
fn workspace_child_exit_worker() {
    let path = PathBuf::from(
        std::env::var_os("REEL_PUBLIC_WORKSPACE_FIXTURE").expect("owned synthetic case"),
    );
    assert_eq!(path.file_name().unwrap(), "case.json");
    let parent = path.parent().unwrap();
    assert!(
        parent
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("reel-public-workspace-fixture-")
    );
    let mut raw = Vec::new();
    File::open(&path)
        .unwrap()
        .take(65537)
        .read_to_end(&mut raw)
        .unwrap();
    assert!(raw.len() <= 65536);
    let case: Case = serde_json::from_slice(&raw).unwrap();
    let ticket = Ticket::decode(&case.ticket, &case.ticket_pin).unwrap();
    assert_eq!(ticket.owner.name, "operation");
    assert_eq!(
        ticket.owner.parent.canonicalize().unwrap(),
        parent.canonicalize().unwrap()
    );
    let preparation = Preparation::restore(&case.preparation, &case.preparation_pin).unwrap();
    let mut ws = Workspace::reopen(&ticket, preparation).unwrap();
    let mut input = source();
    input.exit = true;
    ws.materialize(&mut input).unwrap();
    panic!("expected actual exit");
}

#[test]
#[cfg(windows)]
fn cooperative_time_budget_preserves_incomplete_work_instead_of_looping_forever() {
    struct Interrupted;
    impl Read for Interrupted {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::Interrupted))
        }
    }
    impl std::io::Seek for Interrupted {
        fn seek(&mut self, _: std::io::SeekFrom) -> io::Result<u64> {
            Ok(0)
        }
    }
    impl ByteSource for Interrupted {
        type Reader = Self;
        fn open_verified(&mut self, _: &Object) -> existing::Result<Self> {
            Ok(Self)
        }
    }
    let p = fresh();
    let mut wire: Wire = serde_json::from_slice(p.control()).unwrap();
    wire.budget.millis = 2;
    let parent = tempfile::tempdir().unwrap();
    let (_, mut ws) =
        Workspace::create(parent.path(), "time", Preparation::checked(wire).unwrap()).unwrap();
    assert_eq!(
        ws.materialize(&mut Interrupted).unwrap_err().kind,
        ErrorKind::Budget
    );
    assert!(ws.path().join("owner.json").exists());
}
