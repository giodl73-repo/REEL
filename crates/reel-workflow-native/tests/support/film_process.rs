//! Reuse an exact existing admitted fixture: no new native publication or job.
use super::*;
use reel::scene_delivery::FileRef;
use reel_workflow_native::{
    NativeInputs, NativeProfile, admission,
    film_inputs::{Inputs, Programs},
    film_process::{self, Limits},
};

#[test]
#[ignore = "Explicit original admitted synthetic film fixture and pinned native runtime; contained render/check only, preserves all roots"]
fn contained_movie_render_recovers_without_rendering_again_and_preserves_failed_bounds() {
    let original = PathBuf::from(
        std::env::var_os("REEL_FILM_ADMITTED_FIXTURE").expect("Set REEL_FILM_ADMITTED_FIXTURE"),
    );
    let runtime_path = PathBuf::from(
        std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set REEL_NATIVE_HANDOFF_TOOL"),
    );
    let raw = fs::read(&runtime_path).unwrap();
    let runtime = Tool {
        path: runtime_path,
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    let packet: Packet =
        serde_json::from_slice(&fs::read(original.join("packet.json")).unwrap()).unwrap();
    let source: NativeProfile =
        serde_json::from_slice(&fs::read(original.join("source.json")).unwrap()).unwrap();
    let profile: admission::Profile =
        serde_json::from_slice(&fs::read(original.join("admission-profile.json")).unwrap())
            .unwrap();
    let receipt: admission::Receipt =
        serde_json::from_slice(&fs::read(original.join("admission-receipt.json")).unwrap())
            .unwrap();
    let aliases: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(original.join("aliases.json")).unwrap()).unwrap();
    let programs: Programs =
        serde_json::from_slice(&fs::read(original.join("programs.json")).unwrap()).unwrap();
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let evidence = std::env::var_os("REEL_FILM_EVIDENCE_DIRECTORY").map(PathBuf::from);
    let marker = json!({"root":root,"original_admitted_fixture":original,"runtime":runtime});
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
    eprintln!("Contained film process root: {}", root.display());
    let claims = fs::read(profile.index_root.join("claims.json")).unwrap();
    let native = NativeInputs::open_admitted(&packet, &source, &profile, &receipt).unwrap();
    let inputs = Inputs::prepare(&packet, native, &aliases, programs).unwrap();
    let reference = |name: &str| {
        let asset = packet
            .task
            .inputs
            .iter()
            .find(|a| a.logical_id == aliases[name])
            .unwrap();
        FileRef {
            path: name.into(),
            sha256: asset.sha256.clone(),
            bytes: asset.bytes,
        }
    };
    let job = reference("job.json");
    let production = reference("production.json");
    let limits = Limits {
        timeout_seconds: 120,
        memory_bytes: 4 * 1024 * 1024 * 1024,
        processes: 8,
        workspace_bytes: 128 * 1024 * 1024,
        output_bytes: 32 * 1024 * 1024,
        frames: 48,
        samples: 96000,
        raw_rgb_bytes: 64 * 1024 * 1024,
    };
    let workspace = root.join("render");
    fs::create_dir(&workspace).unwrap();
    let rendered =
        film_process::render(&inputs, &job, &production, &workspace, &runtime, &limits).unwrap();
    assert_eq!(rendered.receipt.delivery_frames, 48);
    assert_eq!(rendered.receipt.plan.review_ids.len(), 2);
    assert_eq!(rendered.receipt.plan.review_ids[1].start_frame, 24);
    let expected = rendered.files.clone();
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(workspace.join("delivery/review.mp4"))
            .is_err()
    );
    assert!(
        film_process::render(&inputs, &job, &production, &workspace, &runtime, &limits).is_err()
    );
    drop(rendered);
    let review = workspace.join("delivery/review.mp4");
    let mut corrupt = expected["review.mp4"].clone();
    corrupt[0] ^= 1;
    fs::write(root.join("refused-review.mp4"), &corrupt).unwrap();
    fs::write(&review, &corrupt).unwrap();
    assert_eq!(
        film_process::recover(&inputs, &job, &production, &workspace, &runtime, &limits)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
    assert!(!workspace.join("check-request.json").exists());
    fs::write(&review, &expected["review.mp4"]).unwrap();
    let recovered =
        film_process::recover(&inputs, &job, &production, &workspace, &runtime, &limits).unwrap();
    assert_eq!(recovered.files, expected);
    let failed = root.join("refused-frames");
    fs::create_dir(&failed).unwrap();
    let mut small = limits.clone();
    small.frames = 47;
    assert_eq!(
        film_process::render(&inputs, &job, &production, &failed, &runtime, &small)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Budget
    );
    assert!(failed.join("render-request.json").is_file());
    assert!(!failed.join("delivery").exists());
    assert!(film_process::render(&inputs, &job, &production, &failed, &runtime, &limits).is_err());
    let timed = root.join("timeout");
    fs::create_dir(&timed).unwrap();
    small = limits.clone();
    small.timeout_seconds = 1;
    let started = std::time::Instant::now();
    assert_eq!(
        film_process::render(&inputs, &job, &production, &timed, &runtime, &small)
            .err()
            .unwrap()
            .kind,
        ErrorKind::NeedsRecovery
    );
    assert!(started.elapsed().as_secs() < 15);
    assert!(timed.join("render-request.json").is_file());
    assert_eq!(
        claims,
        fs::read(profile.index_root.join("claims.json")).unwrap()
    );
    if let Some(e) = evidence {
        fs::write(e.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"original_admitted_fixture":original,
            "frames":48,"cut_frame":24,"rendered_files":expected.len(),"recovered_without_render":true,"output_mutation_refused":true,
            "corrupt_output_refused":true,"frame_budget_refused":true,"timeout_preserved":true,"claims_unchanged":true,
            "output_retained":false,"task_completed":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
