use reel::{
    media_runtime::{Bindings, BoundPrograms, ByteIdentity},
    scene_delivery::FileRef,
};
use std::collections::BTreeMap;
#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
fn identity(path: &Path) -> ByteIdentity {
    ByteIdentity::observe_file(path).unwrap()
}

#[cfg(windows)]
#[test]
fn declared_runtime_verifies_host_and_files_holds_guards_and_omits_private_paths() {
    use reel::scene_delivery::{InputFiles, MediaProgram};
    use std::{fs, path::PathBuf};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let file = |name: &str, bytes: &[u8]| {
        let path = root.join(name);
        fs::write(&path, bytes).unwrap();
        let id = identity(&path);
        FileRef {
            path,
            sha256: id.sha256,
            bytes: id.bytes,
        }
    };
    let bindings = Bindings {
        host: identity(&std::env::current_exe().unwrap()),
        ffmpeg: file("ffmpeg.exe", b"synthetic binary bytes, not executed"),
        ffprobe: file("ffprobe.exe", b"synthetic probe bytes, not executed"),
        dependencies: BTreeMap::from([(
            "libs/codec.dll".into(),
            file("codec.dll", b"declared dependency"),
        )]),
    };
    let bound = BoundPrograms::bind(&bindings).unwrap();
    struct MemoryControl;
    impl InputFiles for MemoryControl {
        fn verified_path(&self, _: &FileRef) -> anyhow::Result<PathBuf> {
            anyhow::bail!("metadata has no filesystem path")
        }
        fn control_bytes(&self, _: &FileRef) -> anyhow::Result<Vec<u8>> {
            Ok(b"literal control".to_vec())
        }
    }
    assert_eq!(
        bound
            .with_inputs(&MemoryControl)
            .control_bytes(&FileRef {
                path: "virtual.json".into(),
                sha256: "0".repeat(64),
                bytes: 15
            })
            .unwrap(),
        b"literal control"
    );
    let components: BTreeMap<_, _> = bound.components().collect();
    assert_eq!(components.len(), 4);
    assert_eq!(components["host"], &bindings.host);
    assert_eq!(components["programs/ffmpeg"].sha256, bindings.ffmpeg.sha256);
    assert_eq!(components["programs/ffprobe"].bytes, bindings.ffprobe.bytes);
    assert_eq!(
        components["dependencies/libs/codec.dll"].sha256,
        bindings.dependencies["libs/codec.dll"].sha256
    );
    let manifest: serde_json::Value = serde_json::from_slice(bound.manifest_bytes()).unwrap();
    assert_eq!(manifest["complete_environment"], false);
    assert_eq!(manifest["host"]["sha256"], bindings.host.sha256);
    assert_eq!(
        manifest["programs"]["ffmpeg"]["bytes"],
        bindings.ffmpeg.bytes
    );
    assert!(!String::from_utf8_lossy(bound.manifest_bytes()).contains(root.to_str().unwrap()));
    let asset = file("asset.dat", b"ordinary input");
    let input = bound.with_inputs(root);
    assert_eq!(
        input.media_program(MediaProgram::Ffmpeg).unwrap(),
        bindings.ffmpeg.path.canonicalize().unwrap()
    );
    assert_eq!(
        input.media_program(MediaProgram::Ffprobe).unwrap(),
        bindings.ffprobe.path.canonicalize().unwrap()
    );
    assert_eq!(
        input
            .verified_path(&FileRef {
                path: PathBuf::from("asset.dat"),
                ..asset
            })
            .unwrap(),
        root.join("asset.dat").canonicalize().unwrap()
    );
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(&bindings.ffmpeg.path)
            .is_err()
    );
    assert!(fs::rename(&bindings.ffmpeg.path, root.join("moved.exe")).is_err());
    assert!(fs::rename(root, root.with_extension("renamed-parent")).is_err());
    assert!(fs::write(&bindings.dependencies["libs/codec.dll"].path, b"change").is_err());
    fs::write(
        root.join("unrelated-writer.dat"),
        b"independent writer remains allowed",
    )
    .unwrap();
    drop(bound);
    fs::write(
        &bindings.ffmpeg.path,
        b"synthetic binary bytes, not executed",
    )
    .unwrap();
    for case in [
        "host", "hash", "size", "relative", "batch", "name", "budget",
    ] {
        let mut bad = bindings.clone();
        match case {
            "host" => bad.host.sha256 = "0".repeat(64),
            "hash" => bad.ffprobe.sha256 = "0".repeat(64),
            "size" => bad.ffprobe.bytes += 1,
            "relative" => bad.ffmpeg.path = "relative.exe".into(),
            "batch" => bad.ffmpeg.path = root.join("implicit-shell.cmd"),
            "name" => {
                bad.dependencies
                    .insert("../escape".into(), bad.ffprobe.clone());
            }
            "budget" => bad.host.bytes = u64::MAX,
            _ => unreachable!(),
        }
        assert!(BoundPrograms::bind(&bad).is_err(), "{case}");
        // A partially bound failure cannot retain write-denying file handles.
        fs::write(
            &bindings.ffmpeg.path,
            b"synthetic binary bytes, not executed",
        )
        .unwrap();
    }
    // Raw content-addressed cache names need no executable suffix or new view.
    let opaque = root.join("opaque-cache-object");
    fs::copy(std::env::current_exe().unwrap(), &opaque).unwrap();
    let mut opaque_bindings = bindings;
    opaque_bindings.ffmpeg = FileRef {
        path: opaque,
        sha256: opaque_bindings.host.sha256.clone(),
        bytes: opaque_bindings.host.bytes,
    };
    let opaque_runtime = BoundPrograms::bind(&opaque_bindings).unwrap();
    let executable = opaque_runtime
        .with_inputs(root)
        .media_program(MediaProgram::Ffmpeg)
        .unwrap();
    assert!(
        std::process::Command::new(executable)
            .arg("--list")
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[cfg(not(windows))]
#[test]
fn mandatory_runtime_protection_refuses_without_cooperative_fallback() {
    let missing = std::env::temp_dir().join("unavailable-reel-runtime/ffmpeg.exe");
    let pin = ByteIdentity {
        sha256: "0".repeat(64),
        bytes: 1,
    };
    let bindings = Bindings {
        host: pin.clone(),
        ffmpeg: FileRef {
            path: missing.clone(),
            sha256: pin.sha256.clone(),
            bytes: 1,
        },
        ffprobe: FileRef {
            path: missing,
            sha256: pin.sha256,
            bytes: 1,
        },
        dependencies: BTreeMap::new(),
    };
    assert!(
        BoundPrograms::bind(&bindings)
            .err()
            .unwrap()
            .to_string()
            .contains("mandatory runtime path protection unavailable")
    );
}
