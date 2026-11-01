//! Recording-clock delivery reuses the existing exact Tig primary-input route.
//! Local job/contract controls remain explicit; this is not full job enrollment.
use super::*;
use reel::scene_delivery;
use tig_core::tree::{MemoryNodes, TreeLimits};

fn select(f: &Fixture, id: &str) {
    let name = format!("records/{}.tig", id.replace('/', "-"));
    let mut record: Value =
        serde_json::from_slice(&fs::read(f.root().join(&name)).unwrap()).unwrap();
    record["status"]["selection_state"] = json!("selected-private-production");
    write(f.root(), &name, &record);
}

fn fixture() -> Fixture {
    let mut f = Fixture::new();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    for id in ["cel/a", "cel/b", "cue/main"] {
        select(&f, id);
    }
    f.commit();
    let production = write(
        f.root(),
        "production.json",
        &json!({"manifest_version":"reel.manifest.v0.2","work":"recording-fixture",
            "title":"Recording","shots":[{"id":"shot","scene_id":"scene"}],
            "audio_events":[{"id":"main","role":"music","source":"cue/main","start_seconds":0}]}),
    );
    let mut contract: Value =
        serde_json::from_slice(&fs::read(f.root().join("contract.json")).unwrap()).unwrap();
    contract["schema"] = json!(reel::cue_relative::AUDIO_EVENT_CONTRACT_SCHEMA);
    let contract = write(f.root(), "contract.json", &contract);
    f.job["contract"] = reference("contract.json", &contract);
    f.job["production_manifest_sha256"] = json!(production.sha256);
    f.job["audio"][0]["bus"] = json!("M");
    f.job["audio"][0].as_object_mut().unwrap().remove("cue_id");
    f.job["buses"]["D"] = json!({"state":"intentional-silence","reason":"No speech"});
    f.job["buses"]["M"] = json!({"state":"present","reason":"Primary recording"});
    write(f.root(), "job.json", &f.job);
    f
}

#[test]
fn current_leaf_change_refuses_stale_job_while_frozen_source_keeps_old_inputs() {
    let mut f = fixture();
    let old = f.source();
    let old_checkpoint = old.checkpoint().clone();
    let mut nodes = MemoryNodes::default();
    let archive = old
        .archive_metadata(&mut nodes, TreeLimits::default())
        .unwrap();
    let old_profile = archived_profile(&f, archive, old_checkpoint.clone());
    let job: Job = serde_json::from_value(f.job.clone()).unwrap();
    let old_b = old.resolve_asset("cel/b").unwrap();
    let changed = f.add("cel/b", &picture([0, 255, 0]));
    select(&f, "cel/b");
    f.commit();
    let mut current = f.source();
    assert!(current.prepare(&job).is_err());
    assert_eq!(current.byte_requests(), 0);
    let mut next_job = job.clone();
    next_job.pictures[1].source = serde_json::from_value(reference("cel/b", &changed)).unwrap();
    let next = current.prepare(&next_job).unwrap();
    assert_eq!(next.objects(), 3);
    drop(next);
    drop(current);
    drop(old);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut restored =
        Source::from_archive(old_profile, &mut nodes, TreeLimits::default()).unwrap();
    assert_eq!(
        restored.resolve_asset("cel/b").unwrap().sha256,
        old_b.sha256
    );
    let inputs = restored.prepare(&job).unwrap();
    assert_eq!(inputs.checkpoint(), &old_checkpoint);
    assert_eq!(inputs.objects(), 3);
    drop(restored);
    let (_, plan) = scene_delivery::plan_with_inputs(&f.root().join("job.json"), &inputs).unwrap();
    assert_eq!(plan.duration_samples, 48000);
    // The source can end, but its exact payload guards still fence mutation.
    assert!(
        fs::write(
            inputs.verified_path(&job.pictures[1].source).unwrap(),
            b"bad"
        )
        .is_err()
    );
    assert!(!f.root().join("exports").exists());
}

#[test]
#[ignore = "requires native FFmpeg; exact Tig recording-clock picture and music PCM"]
fn archived_recording_clock_renders_primary_bytes_without_git_or_narration() {
    let mut f = fixture();
    // Neutral solids avoid asserting RGB preservation through the renderer's
    // declared RGB/YUV conversion; this fixture tests exact temporal selection.
    for (index, id, rgb) in [(0, "cel/a", [0, 0, 0]), (1, "cel/b", [255, 255, 255])] {
        let object = f.add(id, &picture(rgb));
        select(&f, id);
        f.job["pictures"][index]["source"] = reference(id, &object);
    }
    f.commit();
    write(f.root(), "job.json", &f.job);
    let source = f.source();
    let mut nodes = MemoryNodes::default();
    let archive = source
        .archive_metadata(&mut nodes, TreeLimits::default())
        .unwrap();
    let profile = archived_profile(&f, archive, source.checkpoint().clone());
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut restored = Source::from_archive(profile, &mut nodes, TreeLimits::default()).unwrap();
    let job: Job = serde_json::from_value(f.job.clone()).unwrap();
    let inputs = restored.prepare(&job).unwrap();
    drop(restored);
    let output = f.root().join("exports/recording");
    let path = f.root().join("job.json");
    scene_delivery::render_with_inputs(&path, &inputs, &output).unwrap();
    scene_delivery::check_with_inputs(&path, &inputs, &output).unwrap();
    let decode = |name: &str, options: &[&str]| {
        let result = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(output.join(name))
            .args(options)
            .arg("-")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        result.stdout
    };
    assert!(
        decode("M.wav", &["-acodec", "pcm_s24le", "-f", "s24le"]) == wav()[44..],
        "music PCM differs from the exact primary recording"
    );
    let frames = decode(
        "picture.mkv",
        &[
            "-map",
            "0:v:0",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
        ],
    );
    let half = 12 * 64 * 64;
    assert!(
        frames == [[0, 0, 0].repeat(half), [255, 255, 255].repeat(half)].concat(),
        "picture frames differ from the exact ordered neutral-solid sequence"
    );
}
