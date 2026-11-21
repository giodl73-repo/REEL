use super::*;
#[cfg(windows)]
use std::{fs, io::Read};
const UUID: &str = "00000000-0000-0000-0000-000000000001";
mod managed_tests;

fn profile(root: &Path, program: &Path) -> Profile {
    Profile {
        git: program.into(),
        annex: program.into(),
        repository: root.into(),
        bare: false,
        uuid: UUID.into(),
        configuration: Object::from_bytes(b"configuration"),
        tools: vec![program.parent().unwrap().into()],
        empty_config: root.join("empty-config"),
        empty_hooks: root.join("hooks"),
        temporary: root.join("temporary"),
        command_millis: 3000,
        output_bytes: 4096,
        limits: Limits {
            object_bytes: 1024,
            total_bytes: 4096,
            requests: 4,
            live_readers: 2,
            ..Limits::default()
        },
    }
}
#[test]
fn constructor_is_pure_and_rejects_bad_enrollment() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("not-enrolled");
    let mut p = profile(&absent, &root.path().join("absent.exe"));
    assert!(Reader::new(p.clone()).is_ok());
    assert!(!absent.exists());
    p.uuid = "bad".into();
    assert!(matches!(Reader::new(p), Err(Error::Invalid)));
}
#[test]
fn location_and_identity_parsers_refuse_escapes_multiple_lines_and_extensions() {
    let key = "SHA256-s1--hash";
    for bad in [
        "../annex/objects/key",
        "/annex/objects/key",
        ".git/annex/objects/../SHA256-s1--hash",
        ".git/annex/objects/SHA256-s1--hash\nsecond",
        ".git/annex/objects/SHA256E-s1--hash.ext",
    ] {
        assert!(location_path(bad.as_bytes(), ".git/annex/objects/", key).is_err());
    }
    assert!(
        location_path(
            b".git/annex/objects/a/SHA256-s1--hash\n",
            ".git/annex/objects/",
            key
        )
        .is_ok()
    );
    assert!(
        location_path(
            b".\\.git\\annex\\objects\\a\\SHA256-s1--hash\r\n",
            ".git/annex/objects/",
            key
        )
        .is_ok()
    );
    assert!(
        location_path(
            b".git\\annex\\objects\\..\\SHA256-s1--hash\n",
            ".git/annex/objects/",
            key
        )
        .is_err()
    );
    assert!(!uuid("00000000-0000-0000-0000-00000000000X"));
    assert!(line(b"one\ntwo\n").is_err());
}

#[cfg(windows)]
fn fake_tool() -> &'static Path {
    use std::sync::OnceLock;
    static TOOL: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    &TOOL
        .get_or_init(|| {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("fixture.rs");
            let executable = root.path().join("fixture.exe");
            fs::write(
                &source,
                r#"
use std::{fs, io::Write};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args==["heartbeat"] {
        let mut out=fs::OpenOptions::new().append(true).create(true).open("heartbeat").unwrap();
        loop {out.write_all(b"x").unwrap();std::thread::sleep(std::time::Duration::from_millis(10));}
    }
    let mut log=fs::OpenOptions::new().append(true).create(true).open("calls").unwrap();
    writeln!(log,"{}",args.join(" ")).unwrap();
    let mode=fs::read_to_string("mode").unwrap_or_default();
    if mode=="descendant" {
        std::process::Command::new(std::env::current_exe().unwrap()).arg("heartbeat")
            .stdout(std::process::Stdio::inherit()).stderr(std::process::Stdio::inherit()).spawn().unwrap();
        return;
    }
    if mode=="timeout" { std::thread::sleep(std::time::Duration::from_secs(10)); }
    if mode=="oversize" { print!("{}","x".repeat(32768)); return; }
    if args==["config","--local","--no-includes","--get-all","annex.uuid"] {
        if mode=="wrong-uuid" { println!("00000000-0000-0000-0000-000000000002"); }
        else { println!("00000000-0000-0000-0000-000000000001"); } return;
    }
    if args==["config","--local","--no-includes","--get-all","annex.version"] {
        println!("{}",if mode=="old-version" {"7"} else {"10"}); return;
    }
    if args==["config","--local","--no-includes","--get-all","tig.protectionbinding"] {
        print!("{}",fs::read_to_string("binding").unwrap()); return;
    }
    if args.len()==2 && args[0]=="contentlocation" {
        if mode=="missing" {std::process::exit(1);}
        if mode=="observation-error" {eprintln!("fixture error"); std::process::exit(1);}
        if mode=="escape" {println!("../outside/{}",args[1]); return;}
        println!(".git/annex/objects/a/b/{0}/{0}",args[1]); return;
    }
    eprintln!("inadmitted command");std::process::exit(2);
}
"#,
            )
            .unwrap();
            let output = std::process::Command::new("rustc")
                .arg("--edition=2024")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            (root, executable)
        })
        .1
}
#[cfg(windows)]
fn fixture() -> (tempfile::TempDir, Profile, Object, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let p = profile(root.path(), fake_tool());
    fs::create_dir_all(root.path().join(".git/annex/objects/a/b")).unwrap();
    fs::write(root.path().join(".git/config"), b"configuration").unwrap();
    fs::write(&p.empty_config, b"").unwrap();
    fs::create_dir(&p.empty_hooks).unwrap();
    fs::create_dir(&p.temporary).unwrap();
    let object = Object::from_bytes(b"source");
    let key = format!("SHA256-s{}--{}", object.bytes, object.sha256.as_str());
    let dir = root.path().join(".git/annex/objects/a/b").join(&key);
    fs::create_dir(&dir).unwrap();
    let path = dir.join(key);
    fs::write(&path, b"source").unwrap();
    (root, p, object, path)
}
#[cfg(windows)]
#[test]
fn exact_bytes_are_guarded_without_global_info_or_mutating_commands() {
    let (root, p, object, path) = fixture();
    let mut reader = Reader::new(p).unwrap();
    assert!(!root.path().join("calls").exists());
    let mut held = reader.open(&object).unwrap();
    let mut bytes = vec![];
    held.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"source");
    assert!(fs::write(&path, b"change").is_err());
    assert!(fs::remove_file(&path).is_err());
    let calls = fs::read_to_string(root.path().join("calls")).unwrap();
    assert_eq!(calls.lines().count(), 3);
    assert!(!calls.contains("info"));
    assert!(!calls.contains("get "));
    drop(reader);
    assert!(fs::remove_file(&path).is_err());
    drop(held);
    fs::remove_file(path).unwrap();
}
#[cfg(windows)]
#[test]
fn changed_config_and_wrong_identity_refuse_without_payload_fallback() {
    let (root, p, object, _) = fixture();
    fs::write(root.path().join(".git/config"), b"configuratioX").unwrap();
    let mut reader = Reader::new(p).unwrap();
    assert!(matches!(
        reader.open(&object),
        Err(Error::Configuration(existing::Error::Integrity))
    ));
    assert!(!root.path().join("calls").exists());
    let (root, p, object, _) = fixture();
    fs::write(root.path().join("mode"), b"wrong-uuid").unwrap();
    assert!(matches!(
        Reader::new(p).unwrap().open(&object),
        Err(Error::WrongIdentity)
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}
#[cfg(windows)]
#[test]
fn absence_observation_path_and_corruption_remain_distinct() {
    for (mode, expected) in [
        ("missing", Error::Missing),
        ("observation-error", Error::Observation),
        ("escape", Error::Observation),
        ("old-version", Error::Unsupported),
    ] {
        let (root, p, object, _) = fixture();
        fs::write(root.path().join("mode"), mode).unwrap();
        assert!(
            matches!(Reader::new(p).unwrap().open(&object),Err(e) if e==expected),
            "{mode}"
        );
    }
    let (_root, p, object, path) = fixture();
    fs::write(path, b"wrong!").unwrap();
    assert!(matches!(
        Reader::new(p).unwrap().open(&object),
        Err(Error::Bytes(existing::Error::Integrity))
    ));
}
#[cfg(windows)]
#[test]
fn failed_attempts_consume_shared_metadata_budget_and_time_output_are_bounded() {
    let (root, mut p, object, _) = fixture();
    p.limits.requests = 1;
    fs::write(root.path().join("mode"), b"missing").unwrap();
    let mut reader = Reader::new(p).unwrap();
    assert!(matches!(reader.open(&object), Err(Error::Missing)));
    assert!(matches!(
        reader.open(&object),
        Err(Error::Configuration(existing::Error::Budget))
    ));
    assert_eq!(reader.attempts(), 1);
    for mode in ["timeout", "oversize"] {
        let (root, mut p, object, _) = fixture();
        p.command_millis = if mode == "timeout" { 100 } else { 3000 };
        fs::write(root.path().join("mode"), mode).unwrap();
        assert!(Reader::new(p).unwrap().open(&object).is_err());
    }
}

#[cfg(windows)]
#[test]
#[ignore = "requires explicit GIT_ASSET_TEST_GIT and GIT_ASSET_TEST_GIT_ANNEX runtime enrollment"]
fn actual_native_ordinary_and_bare_reads_are_exact_local_and_guarded() {
    let git = PathBuf::from(std::env::var_os("GIT_ASSET_TEST_GIT").expect("explicit Git runtime"));
    let annex = PathBuf::from(
        std::env::var_os("GIT_ASSET_TEST_GIT_ANNEX").expect("explicit annex runtime"),
    );
    let tools = std::env::split_paths(
        &std::env::var_os("GIT_ASSET_TEST_ANNEX_PATH").expect("explicit runtime PATH"),
    )
    .collect::<Vec<_>>();
    for bare in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let mut p = profile(&repo, &git);
        p.git = git.clone();
        p.annex = annex.clone();
        p.tools = tools.clone();
        p.bare = bare;
        p.empty_config = root.path().join("empty-config");
        fs::write(&p.empty_config, b"").unwrap();
        p.empty_hooks = root.path().join("empty-hooks");
        fs::create_dir(&p.empty_hooks).unwrap();
        p.temporary = root.path().join("temporary");
        fs::create_dir(&p.temporary).unwrap();
        p.command_millis = 120000;
        let init = if bare {
            vec!["init", "--bare", "--quiet"]
        } else {
            vec!["init", "--quiet"]
        };
        process::fixture_command(&p, false, &init).unwrap();
        for args in [
            vec!["config", "user.name", "Synthetic Fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
        ] {
            process::fixture_command(&p, false, &args).unwrap();
        }
        process::fixture_command(&p, true, &["init", "synthetic-read-fixture"]).unwrap();
        let object = Object::from_bytes(b"synthetic native bytes");
        let source = repo.join("fixture-source");
        fs::write(&source, b"synthetic native bytes").unwrap();
        let key = format!("SHA256-s{}--{}", object.bytes, object.sha256.as_str());
        process::fixture_command(&p, true, &["setkey", &key, source.to_str().unwrap()]).unwrap();
        let identity =
            process::fixture_command(&p, false, &["config", "--local", "--get", "annex.uuid"])
                .unwrap();
        p.uuid = line(&identity.stdout).unwrap().into();
        let cfg = repo.join(if bare { "config" } else { ".git/config" });
        p.configuration = Object::from_bytes(&fs::read(&cfg).unwrap());
        let before = p.configuration.clone();
        let mut reader = Reader::new(p.clone()).unwrap();
        let mut held = reader.open(&object).unwrap();
        let mut bytes = vec![];
        held.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"synthetic native bytes");
        assert!(
            held.with_path(|path| fs::remove_file(path))
                .unwrap()
                .is_err()
        );
        assert_eq!(Object::from_bytes(&fs::read(&cfg).unwrap()), before);
        drop(held);
        assert!(matches!(
            reader.open(&Object::from_bytes(b"absent")),
            Err(Error::Missing)
        ));
        let (mut managed, namespaces) = managed_tests::roster(p, root.path());
        let binding = managed.binding.id().unwrap();
        process::fixture_command(
            &managed.pool,
            false,
            &[
                "config",
                "--local",
                "tig.protectionbinding",
                binding.as_str(),
            ],
        )
        .unwrap();
        managed.pool.configuration = Object::from_bytes(&fs::read(&cfg).unwrap());
        let expected_config = managed.pool.configuration.clone();
        let mut reader = super::managed::Reader::new(managed).unwrap();
        let mut held = reader.open(&object).unwrap();
        let mut bytes = vec![];
        held.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"synthetic native bytes");
        assert_eq!(held.held_namespaces(), 2);
        assert!(namespaces.iter().all(|n| matches!(
            n.fence(&object),
            Err(git_asset_store::source_claims::admission::Error::Busy)
        )));
        assert!(
            held.with_path(|path| fs::remove_file(path))
                .unwrap()
                .is_err()
        );
        drop(reader);
        drop(held);
        for namespace in namespaces {
            drop(namespace.fence(&object).unwrap());
        }
        assert_eq!(
            Object::from_bytes(&fs::read(&cfg).unwrap()),
            expected_config
        );
    }
}
#[cfg(windows)]
#[test]
fn descendant_held_pipes_are_terminated_and_async_context_refuses_before_io() {
    let (root, mut p, object, _) = fixture();
    p.command_millis = 1000;
    fs::write(root.path().join("mode"), b"descendant").unwrap();
    assert!(matches!(
        Reader::new(p).unwrap().open(&object),
        Err(Error::Observation)
    ));
    let size = fs::metadata(root.path().join("heartbeat")).unwrap().len();
    assert!(size > 0);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        fs::metadata(root.path().join("heartbeat")).unwrap().len(),
        size
    );
    let (root, p, object, _) = fixture();
    let mut reader = Reader::new(p).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        assert!(matches!(reader.open(&object), Err(Error::Unsupported)));
    });
    assert!(!root.path().join("calls").exists());
}
#[cfg(not(windows))]
#[test]
fn unsupported_process_platform_refuses_before_io() {
    let root = tempfile::tempdir().unwrap();
    let p = profile(&root.path().join("absent"), &root.path().join("tool"));
    assert!(matches!(
        Reader::new(p).unwrap().open(&Object::from_bytes(b"x")),
        Err(Error::Unsupported)
    ));
    assert!(!root.path().join("absent").exists());
}
