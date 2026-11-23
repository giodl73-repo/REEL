use fs2::FileExt;
use git_asset_core::Object;
use git_asset_store::existing::*;
#[cfg(windows)]
use std::io::Write;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn policy() -> RequiredProtection {
    if cfg!(windows) {
        RequiredProtection::PreventMutation
    } else {
        RequiredProtection::CooperativeImmutable
    }
}
fn write(root: &Path, layout: &Layout, data: &[u8]) -> Object {
    let object = Object::from_bytes(data);
    let path = layout.object_path(root, &object.sha256);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
    object
}
fn profile(root: &Path, layout: Layout) -> Profile {
    Profile {
        places: vec![Place {
            root: root.to_owned(),
            layout,
        }],
        required: policy(),
        limits: Limits::default(),
    }
}

#[test]
fn every_existing_layout_verifies_and_rewinds_a_held_reader() {
    for layout in [Layout::ReelCas, Layout::GitLfs, Layout::Flat] {
        let t = tempfile::tempdir().unwrap();
        let object = write(t.path(), &layout, b"pixels");
        let mut cache = ReadCache::new(profile(t.path(), layout)).unwrap();
        let mut reader = cache.open_verified(&object).unwrap();
        assert_eq!(reader.object(), &object);
        assert_eq!(reader.place_index(), 0);
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"pixels");
        reader.seek(SeekFrom::Start(1)).unwrap();
        let mut out = String::new();
        reader.read_to_string(&mut out).unwrap();
        assert_eq!(out, "ixels");
        assert_eq!(cache.active_readers(), 1);
        drop(reader);
        assert_eq!(cache.active_readers(), 0);
    }
}

#[cfg(windows)]
#[test]
fn canonical_windows_roots_preserve_layout_missing_and_guard_semantics() {
    for layout in [Layout::ReelCas, Layout::GitLfs, Layout::Flat] {
        let temp = tempfile::tempdir().unwrap();
        let object = write(temp.path(), &layout, b"canonical existing bytes");
        let canonical = temp.path().canonicalize().unwrap();
        for root in [temp.path().to_owned(), canonical] {
            let path = layout.object_path(&root, &object.sha256);
            let mut cache = ReadCache::new(profile(&root, layout.clone())).unwrap();
            let missing = Object::from_bytes(b"absent object");
            assert!(matches!(cache.open_verified(&missing), Err(Error::Missing)));
            let mut held = cache.open_verified(&object).unwrap();
            let mut bytes = Vec::new();
            held.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"canonical existing bytes");
            assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
            assert!(fs::rename(&path, path.with_extension("moved")).is_err());
            fs::write(root.join("independent-writer"), b"allowed").unwrap();
            drop(held);
            fs::write(&path, b"canonical existing bytes").unwrap();
        }
    }
}

#[cfg(windows)]
#[test]
fn incomplete_windows_prefixes_are_not_enrolled_as_absolute_roots() {
    for root in [r"C:", r"C:relative", r"\\?\C:", r"\\server\share\cache"] {
        assert!(matches!(
            ReadCache::new(profile(Path::new(root), Layout::Flat)),
            Err(Error::Invalid)
        ));
    }
}

#[test]
fn missing_roots_fall_back_without_creation_but_corrupt_present_roots_do_not() {
    let t = tempfile::tempdir().unwrap();
    let absent = t.path().join("absent");
    let warm = t.path().join("warm");
    let cold = t.path().join("cold");
    let object = write(&cold, &Layout::ReelCas, b"good");
    let mut p = profile(&absent, Layout::ReelCas);
    p.places.push(Place {
        root: cold,
        layout: Layout::ReelCas,
    });
    let mut cache = ReadCache::new(p.clone()).unwrap();
    assert_eq!(cache.open_verified(&object).unwrap().place_index(), 1);
    assert!(!absent.exists());
    let path = Layout::ReelCas.object_path(&warm, &object.sha256);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"evil").unwrap();
    p.places.insert(
        0,
        Place {
            root: warm,
            layout: Layout::ReelCas,
        },
    );
    let mut cache = ReadCache::new(p).unwrap();
    assert!(matches!(
        cache.open_verified(&object),
        Err(Error::Integrity)
    ));
    assert_eq!(cache.verification_bytes(), 4);
    assert_eq!(cache.active_readers(), 0);
}

#[test]
fn wrong_size_and_directory_objects_never_report_verified() {
    let t = tempfile::tempdir().unwrap();
    let object = write(t.path(), &Layout::Flat, b"one");
    let path = Layout::Flat.object_path(t.path(), &object.sha256);
    fs::write(&path, b"too long").unwrap();
    let mut cache = ReadCache::new(profile(t.path(), Layout::Flat)).unwrap();
    assert!(matches!(
        cache.open_verified(&object),
        Err(Error::Integrity)
    ));
    assert_eq!(cache.verification_bytes(), 0);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(matches!(
        cache.open_verified(&object),
        Err(Error::Integrity)
    ));
}

#[test]
fn invalid_and_bounded_profiles_refuse_without_creating_files() {
    let t = tempfile::tempdir().unwrap();
    let mut p = profile(&t.path().join("missing"), Layout::Flat);
    p.places[0].root = "relative".into();
    assert!(matches!(ReadCache::new(p), Err(Error::Invalid)));
    let mut p = profile(t.path(), Layout::Flat);
    p.limits.live_readers = 0;
    assert!(matches!(ReadCache::new(p), Err(Error::Invalid)));
    let object = Object::from_bytes(b"data");
    let mut p = profile(t.path(), Layout::Flat);
    p.limits.object_bytes = 1;
    let mut cache = ReadCache::new(p).unwrap();
    assert!(matches!(cache.open_verified(&object), Err(Error::Budget)));
    assert_eq!(cache.requests(), 0);
    assert!(fs::read_dir(t.path()).unwrap().next().is_none());
}

#[test]
fn request_byte_and_live_reader_budgets_are_enforced_and_released() {
    let t = tempfile::tempdir().unwrap();
    let object = write(t.path(), &Layout::Flat, b"data");
    let mut p = profile(t.path(), Layout::Flat);
    p.limits.live_readers = 1;
    p.limits.total_bytes = 8;
    p.limits.requests = 4;
    let mut cache = ReadCache::new(p).unwrap();
    let held = cache.open_verified(&object).unwrap();
    assert!(matches!(cache.open_verified(&object), Err(Error::Budget)));
    assert_eq!(cache.active_readers(), 1);
    drop(held);
    drop(cache.open_verified(&object).unwrap());
    assert_eq!(cache.verification_bytes(), 8);
    assert!(matches!(cache.open_verified(&object), Err(Error::Budget)));
}

#[test]
fn empty_payloads_and_missing_requests_have_explicit_results() {
    let t = tempfile::tempdir().unwrap();
    let object = write(t.path(), &Layout::Flat, b"");
    let mut p = profile(t.path(), Layout::Flat);
    p.limits.requests = 2;
    let mut cache = ReadCache::new(p).unwrap();
    drop(cache.open_verified(&object).unwrap());
    assert!(matches!(
        cache.open_verified(&Object::from_bytes(b"missing")),
        Err(Error::Missing)
    ));
    assert!(matches!(cache.open_verified(&object), Err(Error::Budget)));
    assert_eq!(cache.active_readers(), 0);
}

#[test]
fn cooperative_handle_allows_alias_lifecycle_and_excludes_cooperating_writers() {
    let t = tempfile::tempdir().unwrap();
    let object = write(t.path(), &Layout::ReelCas, b"data");
    let source = Layout::ReelCas.object_path(t.path(), &object.sha256);
    let mut p = profile(t.path(), Layout::ReelCas);
    p.required = RequiredProtection::CooperativeImmutable;
    let mut cache = ReadCache::new(p).unwrap();
    let mut held = cache.open_verified(&object).unwrap();
    assert_eq!(held.protection(), Protection::CooperativeFileLock);
    let alias = t.path().join("alias");
    let moved = t.path().join("moved");
    fs::hard_link(&source, &alias).unwrap();
    fs::rename(&alias, &moved).unwrap();
    fs::remove_file(moved).unwrap();
    let contender = fs::File::open(source).unwrap();
    assert!(FileExt::try_lock_exclusive(&contender).is_err());
    assert!(matches!(held.with_path(|_| ()), Err(Error::Unsupported)));
    let mut out = String::new();
    held.read_to_string(&mut out).unwrap();
    assert_eq!(out, "data");
    drop(held);
    FileExt::try_lock_exclusive(&contender).unwrap();
}

#[cfg(windows)]
#[test]
fn protected_path_blocks_object_mutation_and_parent_rename_but_other_objects_progress() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("cache");
    let object = write(&root, &Layout::Flat, b"data");
    let source = Layout::Flat.object_path(&root, &object.sha256);
    let mut cache = ReadCache::new(profile(&root, Layout::Flat)).unwrap();
    let held = cache.open_verified(&object).unwrap();
    assert_eq!(held.protection(), Protection::WindowsSharing);
    assert!(fs::OpenOptions::new().write(true).open(&source).is_err());
    assert!(fs::remove_file(&source).is_err());
    assert!(fs::rename(&root, t.path().join("moved-cache")).is_err());
    let other = write(&root, &Layout::Flat, b"unrelated");
    fs::remove_file(Layout::Flat.object_path(&root, &other.sha256)).unwrap();
    assert_eq!(held.with_path(|p| fs::read(p).unwrap()).unwrap(), b"data");
    drop(held);
    fs::remove_file(source).unwrap();
}

#[cfg(not(windows))]
#[test]
fn required_mandatory_exclusion_never_downgrades_on_unix() {
    let t = tempfile::tempdir().unwrap();
    let mut p = profile(t.path(), Layout::Flat);
    p.required = RequiredProtection::PreventMutation;
    let mut cache = ReadCache::new(p).unwrap();
    assert!(matches!(
        cache.open_verified(&Object::from_bytes(b"missing")),
        Err(Error::Unsupported)
    ));
    assert_eq!(cache.requests(), 0);
}

#[cfg(unix)]
#[test]
fn symlink_objects_and_roots_are_rejected_without_following_them() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("cache");
    let object = write(&root, &Layout::Flat, b"data");
    let path = Layout::Flat.object_path(&root, &object.sha256);
    fs::remove_file(&path).unwrap();
    let target = t.path().join("outside");
    fs::write(&target, b"data").unwrap();
    symlink(&target, &path).unwrap();
    let mut cache = ReadCache::new(profile(&root, Layout::Flat)).unwrap();
    assert!(matches!(
        cache.open_verified(&object),
        Err(Error::Integrity)
    ));
    let linked = t.path().join("linked");
    symlink(&root, &linked).unwrap();
    let mut cache = ReadCache::new(profile(&linked, Layout::Flat)).unwrap();
    assert!(matches!(
        cache.open_verified(&object),
        Err(Error::Integrity)
    ));
}

#[test]
fn read_helper() {
    let Some(root) = std::env::var_os("GIT_ASSET_EXISTING_READ_TEST") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mut cache = ReadCache::new(profile(&root.join("cache"), Layout::Flat)).unwrap();
    let _held = cache.open_verified(&Object::from_bytes(b"data")).unwrap();
    fs::write(root.join("ready"), b"ready").unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}

#[test]
fn actual_child_exit_releases_effective_object_read_protection() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("cache");
    let object = write(&root, &Layout::Flat, b"data");
    let path = Layout::Flat.object_path(&root, &object.sha256);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "read_helper", "--nocapture"])
        .env("GIT_ASSET_EXISTING_READ_TEST", t.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while !t.path().join("ready").exists() {
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child guard setup deadline");
        }
        assert!(child.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(2));
    }
    #[cfg(windows)]
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    let file = fs::File::open(&path).unwrap();
    assert!(FileExt::try_lock_exclusive(&file).is_err());
    child.kill().unwrap();
    child.wait().unwrap();
    FileExt::try_lock_exclusive(&file).unwrap();
    #[cfg(windows)]
    {
        drop(file);
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .write_all(b"safe")
            .unwrap();
    }
}

#[test]
fn ending_one_cooperative_guard_preserves_another_independent_reader() {
    let temp = tempfile::tempdir().unwrap();
    let object = write(temp.path(), &Layout::Flat, b"independent readers");
    let source = Layout::Flat.object_path(temp.path(), &object.sha256);
    let mut p = profile(temp.path(), Layout::Flat);
    p.required = RequiredProtection::CooperativeImmutable;
    let mut cache = ReadCache::new(p).unwrap();
    let first = cache.open_verified(&object).unwrap();
    let second = cache.open_verified(&object).unwrap();
    let contender = fs::File::open(source).unwrap();
    drop(first);
    assert_eq!(cache.active_readers(), 1);
    assert!(FileExt::try_lock_exclusive(&contender).is_err());
    drop(second);
    assert_eq!(cache.active_readers(), 0);
    FileExt::try_lock_exclusive(&contender).unwrap();
}
