use super::*;

fn limits() -> Limits {
    Limits {
        timeout_seconds: 120,
        memory_bytes: 2 * 1024 * 1024 * 1024,
        processes: 16,
        workspace_bytes: 32 * 1024 * 1024,
        output_bytes: 16 * 1024 * 1024,
        frames: 24,
        samples: 48000,
        raw_rgb_bytes: 16 * 1024 * 1024,
    }
}
fn reference(path: &Path, name: &str) -> FileRef {
    let raw = fs::read(path).unwrap();
    FileRef {
        path: name.into(),
        sha256: reel_workflow::digest(&raw),
        bytes: raw.len() as u64,
    }
}
fn tool(path: PathBuf) -> Tool {
    let pin = reference(&path, "runtime");
    Tool {
        path,
        sha256: pin.sha256,
        bytes: pin.bytes,
    }
}

#[test]
fn recovery_distinguishes_an_incomplete_render_from_corrupt_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for partially_written in [false, true] {
        let output = root.join("delivery");
        if partially_written {
            fs::create_dir(&output).unwrap();
            fs::write(output.join("partial.mp4"), b"partial").unwrap();
        }
        let err = collect_recovered_render(&root, &limits()).err().unwrap();
        assert_eq!(err.kind, ErrorKind::NeedsRecovery);
        assert_eq!(err.code, "film-process-incomplete-render");
    }
    let output = root.join("delivery");
    assert_eq!(fs::read(output.join("partial.mp4")).unwrap(), b"partial");
    fs::write(output.join("receipt.json"), b"{").unwrap();
    let err = collect_recovered_render(&root, &limits()).err().unwrap();
    assert_eq!(err.kind, ErrorKind::Integrity);
    assert_eq!(err.code, "film-process-receipt");
}

#[test]
fn declared_five_hour_large_master_bounds_are_distinct_from_admission() {
    let mut budget = limits();
    budget.timeout_seconds = 18_000;
    budget.output_bytes = 4 * 1024 * 1024 * 1024;
    budget.workspace_bytes = 16 * 1024 * 1024 * 1024;
    budget.samples = 48_000 * 18_000;
    budget.validate().unwrap();
    budget.timeout_seconds += 1;
    assert_eq!(budget.validate().unwrap_err().kind, ErrorKind::Budget);
}

#[test]
fn output_above_old_128_mib_bound_is_streamed_and_guarded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("synthetic.bin");
    let chunk = [73u8; 65536];
    let mut file = File::create(&path).unwrap();
    let mut hash = Sha256::new();
    for _ in 0..2049 {
        file.write_all(&chunk).unwrap();
        hash.update(chunk);
    }
    file.sync_all().unwrap();
    drop(file);
    let pin = FileRef {
        path: "synthetic.bin".into(),
        bytes: 2049 * 65536,
        sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
    };
    let mut output = sealed(&path, pin).unwrap();
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    struct Count(u64);
    impl Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            assert!(bytes.len() <= 65536);
            self.0 += bytes.len() as u64;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    output.copy_to(&mut count).unwrap();
    assert_eq!(count.0, output.reference().bytes);
    let mut wrong = output.reference().clone();
    wrong.sha256 = "0".repeat(64);
    assert!(sealed(&path, wrong).is_err());
}

#[path = "../tests/support/film_job.rs"]
mod film_job;
use film_job::fixture;

#[test]
#[ignore = "Requires explicitly pinned test CLI, FFmpeg and FFprobe; renders only owned synthetic media, no native pool or project processing"]
fn contained_renderer_recovers_by_checking_without_rerendering_and_preserves_failed_intent() {
    let temp = tempfile::tempdir().unwrap();
    let inputs_root = temp.path().join("inputs");
    fs::create_dir(&inputs_root).unwrap();
    let workspace = temp.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let inputs = fixture(&inputs_root);
    let program = |name| {
        tool(
            std::env::var_os(name)
                .expect("Set explicit film test runtime paths")
                .into(),
        )
    };
    let mut request = Request {
        schema: "reel.workflow-film-process.v2".into(),
        task_key: "a".repeat(64),
        action: Action::Render,
        job: reference(&inputs_root.join("job.json"), "job.json"),
        production: reference(&inputs_root.join("production.json"), "production.json"),
        inputs,
        programs: Programs {
            ffmpeg: program("REEL_TEST_FFMPEG"),
            ffprobe: program("REEL_TEST_FFPROBE"),
        },
        runtime: program("REEL_FILM_TEST_RUNTIME"),
        workspace: workspace.clone(),
        limits: limits(),
    };
    let mut proof = launch(&request, "render-request.json").unwrap();
    let rendered = collect(&workspace, &request.limits).unwrap();
    assert_eq!(rendered.receipt.content_samples, 4800);
    assert_eq!(rendered.receipt.delivery_frames, 3);
    check_output_proof(&rendered, &proof).unwrap();
    proof.get_mut("receipt.json").unwrap().sha256 = "0".repeat(64);
    assert_eq!(
        check_output_proof(&rendered, &proof).unwrap_err().code,
        "film-process-child-output-binding"
    );
    assert!(rendered.files.contains_key("master.mkv"));
    let before: BTreeMap<_, _> = rendered
        .files
        .iter()
        .map(|(name, output)| (name.clone(), output.reference().clone()))
        .collect();
    request.action = Action::Check;
    launch(&request, "check-request.json").unwrap();
    let checked = collect(&workspace, &request.limits).unwrap();
    let checked_proof = reconcile_check(&request).unwrap();
    check_output_proof(&checked, &checked_proof).unwrap();
    let render_request = fs::read(workspace.join("render-request.json")).unwrap();
    let render_result = workspace.join("render-result.json");
    let original = fs::read(&render_result).unwrap();
    preserve_recovered_outcome(&workspace, &render_request, &request, &checked_proof).unwrap();
    assert_eq!(fs::read(&render_result).unwrap(), original);

    fs::remove_file(&render_result).unwrap();
    preserve_recovered_outcome(&workspace, &render_request, &request, &checked_proof).unwrap();
    let recovered_bytes = fs::read(&render_result).unwrap();
    preserve_recovered_outcome(&workspace, &render_request, &request, &checked_proof).unwrap();
    assert_eq!(fs::read(&render_result).unwrap(), recovered_bytes);
    let recovered: serde_json::Value = serde_json::from_slice(&recovered_bytes).unwrap();
    assert_eq!(
        recovered["schema"],
        "reel.workflow-film-process-recovered-outcome.v1"
    );
    assert_eq!(
        recovered["request_sha256"],
        reel_workflow::digest(&render_request)
    );
    assert_eq!(
        recovered["check_request_sha256"],
        reel_workflow::digest(&fs::read(workspace.join("check-request.json")).unwrap())
    );
    assert_eq!(
        recovered["outputs"],
        serde_json::to_value(&checked_proof).unwrap()
    );
    fs::write(&render_result, b"{").unwrap();
    assert!(
        preserve_recovered_outcome(&workspace, &render_request, &request, &checked_proof).is_err()
    );
    assert_eq!(fs::read(&render_result).unwrap(), b"{");
    for (name, output) in &checked.files {
        assert_eq!(before[name].sha256, output.reference().sha256);
    }
    assert_eq!(
        launch(&request, "check-request.json").unwrap_err().code,
        "film-process-original-launch-exists"
    );
    assert!(workspace.join("render-request.json").is_file());
    fs::write(workspace.join("delivery/unexpected.bin"), b"extra").unwrap();
    assert_eq!(
        collect(&workspace, &request.limits).err().unwrap().code,
        "film-process-output-inventory"
    );
    let failed = temp.path().join("failed");
    fs::create_dir(&failed).unwrap();
    request.action = Action::Render;
    request.workspace = failed.clone();
    request.programs.ffmpeg = request.runtime.clone();
    assert_eq!(
        launch(&request, "render-request.json").unwrap_err().code,
        "film-process-render-preserve"
    );
    assert!(failed.join("render-request.json").is_file());
    assert!(failed.join("render-result.json").is_file());
    assert!(fs::read_dir(&failed).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".scene-delivery-")
    }));
    assert_eq!(
        launch(&request, "render-request.json").unwrap_err().code,
        "film-process-original-launch-exists"
    );
}
