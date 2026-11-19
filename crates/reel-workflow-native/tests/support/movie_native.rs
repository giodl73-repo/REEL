//! Original movie -> managed candidate publication/retention -> native history.
use super::movie_verify::{Metadata, package, tool};
use super::*;
use reel_workflow_native::{
    MovieVerifier, NativeProfile, admission, film_inputs::Programs, response, retention, staging,
};
fn phase(root: &Path, name: &str) {
    fs::write(
        root.join("phase.json"),
        serde_json::to_vec(&json!({"phase":name})).unwrap(),
    )
    .unwrap();
}
fn withhold_files(root: &Path) -> Vec<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    let mut pending = vec![root.to_path_buf()];
    let mut held = Vec::new();
    while let Some(at) = pending.pop() {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            assert!(!meta.file_type().is_symlink());
            if meta.is_dir() {
                pending.push(path);
            } else {
                held.push(
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(path)
                        .unwrap(),
                );
            }
        }
    }
    held
}
#[test]
#[ignore = "Original admitted movie and existing native pool only; managed response/retention and bare-history verification, preserves operation roots"]
fn movie_candidate_retains_and_verifies_from_native_bytes_without_worker_media() {
    let original = PathBuf::from(
        std::env::var_os("REEL_MOVIE_RECOVERY_FIXTURE").expect("Set original movie fixture"),
    );
    let packet: Packet =
        serde_json::from_slice(&fs::read(original.join("packet.json")).unwrap()).unwrap();
    let mut source: NativeProfile =
        serde_json::from_slice(&fs::read(original.join("source.json")).unwrap()).unwrap();
    let admission: admission::Profile =
        serde_json::from_slice(&fs::read(original.join("admission-profile.json")).unwrap())
            .unwrap();
    let receipt: admission::Receipt =
        serde_json::from_slice(&fs::read(original.join("admission-receipt.json")).unwrap())
            .unwrap();
    let old_ticket: staging::Ticket =
        serde_json::from_slice(&fs::read(original.join("movie-owned-ticket.json")).unwrap())
            .unwrap();
    let programs: Programs =
        serde_json::from_slice(&fs::read(original.join("programs.json")).unwrap()).unwrap();
    let verifier = tool(PathBuf::from(
        std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set verifier runtime"),
    ));
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let evidence = std::env::var_os("REEL_FILM_EVIDENCE_DIRECTORY").map(PathBuf::from);
    let marker = json!({"root":root,"original":original,"task_key":packet.task_key,"work":receipt.work,"verifier":verifier});
    fs::write(
        root.join("original-operation.json"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    if let Some(e) = &evidence {
        fs::write(
            e.join("original-operation-roots.json"),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
    }
    let parent = root.join("checks");
    fs::create_dir(&parent).unwrap();
    source.movie_verifier = Some(MovieVerifier {
        runtime: verifier,
        auxiliary_programs: BTreeMap::from([
            ("ffmpeg".into(), programs.ffmpeg),
            ("ffprobe".into(), programs.ffprobe),
        ]),
        staging: staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent: parent.clone(),
            maximum_bytes: 256 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 100 * 1024 * 1024,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        },
    });
    let bound = film_render::bind(&packet, &mut Metadata(&original)).unwrap();
    let worker = old_ticket.directory().join("movie");
    let files = bound
        .request()
        .output_paths
        .values()
        .map(|p| (p.clone(), fs::read(worker.join(p)).unwrap()))
        .collect();
    let (result, returns) = package(
        &packet,
        &bound,
        &files,
        &root.join("returns"),
        &receipt.attempt_id,
    );
    let target = retained_target();
    for (name, raw) in [
        ("packet.json", serde_json::to_vec(&packet).unwrap()),
        ("source-profile.json", serde_json::to_vec(&source).unwrap()),
        (
            "admission-profile.json",
            serde_json::to_vec(&admission).unwrap(),
        ),
        (
            "admission-receipt.json",
            serde_json::to_vec(&receipt).unwrap(),
        ),
        ("result.json", serde_json::to_vec(&result).unwrap()),
        ("returns.json", serde_json::to_vec(&returns).unwrap()),
        ("target.json", serde_json::to_vec(&target).unwrap()),
    ] {
        fs::write(root.join(name), raw).unwrap();
    }
    phase(root, "ingest-original-result");
    let candidate =
        response::ingest(&packet, &source, &admission, &receipt, &result, &returns).unwrap();
    fs::write(
        root.join("candidate.json"),
        serde_json::to_vec(&candidate).unwrap(),
    )
    .unwrap();
    phase(root, "retain-original-candidate");
    let acknowledgement =
        retention::retain(&packet, &source, &admission, &receipt, &result, &target).unwrap();
    fs::write(
        root.join("ack.json"),
        acknowledgement.canonical_bytes().unwrap(),
    )
    .unwrap();
    phase(root, "verify-retained");
    drop(
        retention::verify(
            &packet,
            &source,
            &admission,
            &receipt,
            &result,
            &acknowledgement,
        )
        .unwrap(),
    );
    let claims = fs::read(admission.index_root.join("claims.json")).unwrap();
    // Move only this fixture's caller-owned return directory, preserving every
    // byte. Both resolved targets remain beneath this explicitly owned root.
    let canonical = root.canonicalize().unwrap();
    let from = returns.root.canonicalize().unwrap();
    let to = canonical.join("returns-preserved");
    assert!(from.starts_with(&canonical));
    assert_eq!(to.parent(), Some(canonical.as_path()));
    assert!(!to.exists());
    fs::rename(&from, &to).unwrap();
    assert!(!returns.root.exists());
    let withheld = withhold_files(&worker);
    assert!(!withheld.is_empty());
    assert!(
        fs::read(worker.join("delivery/master.mkv")).is_err(),
        "original worker media is still readable"
    );
    let history = root.join("history.git");
    let cloned = Command::new(&source.snapshot.git.path)
        .args(["clone", "--bare", "--no-hardlinks"])
        .arg(&source.snapshot.repository)
        .arg(&history)
        .output()
        .unwrap();
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    source.snapshot.repository = history;
    fs::write(
        root.join("historical-source.json"),
        serde_json::to_vec(&source).unwrap(),
    )
    .unwrap();
    phase(root, "verify-native-without-return-or-worker-media");
    drop(
        retention::verify(
            &packet,
            &source,
            &admission,
            &receipt,
            &result,
            &acknowledgement,
        )
        .unwrap(),
    );
    assert_eq!(
        claims,
        fs::read(admission.index_root.join("claims.json")).unwrap()
    );
    let contexts = fs::read_dir(&parent)
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            e.file_type().unwrap().is_dir().then_some(e.path())
        })
        .collect::<Vec<_>>();
    // Return verification and native verification each reuse their own original
    // admitted context; repeated retention checks do not allocate fresh scratch.
    assert_eq!(contexts.len(), 2);
    phase(root, "retained-history-verified");
    if let Some(e) = evidence {
        fs::write(e.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"original":original,"task_key":packet.task_key,
        "candidate":candidate,"retained":acknowledgement,"outputs":result.artifacts.len(),"frames":48,"owned_verification_contexts":2,
        "original_admission_reused":true,"native_retained_movie_verified":true,"bare_history_verified":true,"return_directory_absent":true,
        "worker_media_read_denied":true,"claims_unchanged_on_history_read":true,"task_completed":false,"selected":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
