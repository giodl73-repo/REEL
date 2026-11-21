use super::*;
use std::process::Command;
use tig_core::Object;
use tig_git::pointer::Pointer;
fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new(executable());
    for (name, _) in std::env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(name);
        }
    }
    let output = command
        .arg("-C")
        .arg(repo)
        .args(args)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn executable() -> std::path::PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join(format!("git{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_absolute() && p.is_file())
        .unwrap()
}
fn profile(root: &Path) -> serde_json::Value {
    let repo = root.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["-c", "init.templateDir=", "init", "-b", "main"]);
    git(&repo, &["config", "core.hooksPath", "no-hooks"]);
    git(&repo, &["config", "user.name", "Public fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.invalid"]);
    fs::create_dir(repo.join("documents")).unwrap();
    fs::write(
        repo.join("documents/specimen.tig"),
        Pointer::new(Object::from_bytes(b"synthetic non-media specimen")).to_bytes(),
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial specimen"]);
    let commit = String::from_utf8(git(&repo, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_string();
    json!({"schema":"tig.source-init-profile.experimental-1","root":root.join("control"),"operation":"init-specimen","project":"public-specimen","source":"main",
        "index_epoch":"index-epoch","journal_epoch":"source-epoch","controller":"controller","subject":Digest::blob(b"subject"),"policies":[Digest::blob(b"policy")],
        "git_executable":executable(),"repository":repo,"binding":{"repository":Digest::blob(b"repo"),"reference":"refs/heads/main","actor":"main","owner":"owner","scope":["documents"]},
        "commit":commit,"main_stores":[Digest::blob(b"F")],"companions":[],"max_bindings":1000,
        "metadata_limits":{"nodes":10000,"bytes":16777216,"database_bytes":33554432,"min_free_bytes":65536}})
}
#[test]
fn process_cut_child() {
    let Some(file) = std::env::var_os("TIG_SOURCE_BOOTSTRAP_PROFILE") else {
        return;
    };
    let stage = std::env::var("TIG_SOURCE_BOOTSTRAP_CUT").unwrap();
    let p = crate::init_profile::parse(&fs::read(file).unwrap()).unwrap();
    let _ = initialize_hook(p, |s| {
        if format!("{s:?}") == stage {
            std::process::exit(73)
        }
        Ok(())
    });
    panic!("requested source bootstrap cut did not execute")
}
#[test]
fn eight_actual_process_exits_resume_exact_source_initialization_from_durable_progress() {
    for stage in [
        Stage::ProgressReady,
        Stage::MetadataReady,
        Stage::SelectionsReady,
        Stage::IndexReady,
        Stage::IndexBootstrapped,
        Stage::JournalReady,
        Stage::Enrolled,
        Stage::Published,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let value = profile(temp.path());
        let input = temp.path().join("profile.json");
        fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "initialize::tests::process_cut_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("TIG_SOURCE_BOOTSTRAP_PROFILE", &input)
            .env("TIG_SOURCE_BOOTSTRAP_CUT", format!("{stage:?}"))
            .output()
            .unwrap();
        assert_eq!(
            child.status.code(),
            Some(73),
            "{stage:?}: {} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        let p = crate::init_profile::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        let mut canonical = p.clone();
        canonical.root = initialization::canonical_root(&p.root).unwrap();
        let intent = Digest::blob(
            &encode(&json!([
                "tig.source-initialization-intent.experimental-1",
                &canonical
            ]))
            .unwrap(),
        );
        let ns = Namespace::new(&p.root, intent).unwrap();
        let path = if stage == Stage::Published {
            p.root.join("source-init.json")
        } else {
            ns.stage().join("source-init.json")
        };
        let before: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        if !matches!(stage, Stage::ProgressReady | Stage::MetadataReady) {
            fs::rename(temp.path().join("repo"), temp.path().join("offline-repo")).unwrap();
        }
        let output: serde_json::Value =
            serde_json::from_slice(&initialize(p.clone()).unwrap()).unwrap();
        for name in ["index", "journal"] {
            if !before[name].is_null() {
                assert_eq!(before[name]["identity"], output[name]["identity"]);
            }
        }
        if !before["metadata"].is_null() {
            assert_eq!(
                before["metadata"]["identity"],
                output["metadata"]["minimum_checkpoint"]["identity"]
            );
        }
        assert_eq!(output["actors"]["main"]["revision"], 1);
        assert_eq!(output["placement_executed"], false);
        assert!(p.root.is_dir());
        assert!(!ns.stage().exists());
        let replay: serde_json::Value = serde_json::from_slice(&initialize(p).unwrap()).unwrap();
        assert_eq!(output, replay);
    }
}
