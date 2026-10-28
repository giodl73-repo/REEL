use reel::{
    media_parity::{Shape, compare_with_inputs},
    scene_delivery::FileRef,
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};
fn ffmpeg(root: &Path, args: &[&str]) {
    let output = Command::new("ffmpeg")
        .current_dir(root)
        .args(["-v", "error", "-nostdin"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn reference(root: &Path, name: &str) -> FileRef {
    let bytes = fs::read(root.join(name)).unwrap();
    FileRef {
        path: name.into(),
        bytes: bytes.len() as u64,
        sha256: Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
}
#[test]
fn explicit_validation_program_refusal_precedes_payload_access_without_fallback() {
    use reel::scene_delivery::{InputFiles, MediaProgram};
    struct Refuse(std::cell::Cell<usize>);
    impl InputFiles for Refuse {
        fn verified_path(&self, _: &FileRef) -> anyhow::Result<std::path::PathBuf> {
            self.0.set(self.0.get() + 1);
            anyhow::bail!("unexpected payload access")
        }
        fn media_program(&self, _: MediaProgram) -> anyhow::Result<std::path::PathBuf> {
            anyhow::bail!("explicit validation program refusal")
        }
    }
    let source = Refuse(std::cell::Cell::new(0));
    let file = FileRef {
        path: "unopened.mkv".into(),
        sha256: "0".repeat(64),
        bytes: 1,
    };
    let shape = Shape {
        width: 32,
        height: 32,
        fps_numerator: 24,
        fps_denominator: 1,
        frames: 24,
        sample_rate: 48000,
        samples: 48000,
    };
    assert!(
        compare_with_inputs(&file, &source, &file, &source, &shape)
            .unwrap_err()
            .to_string()
            .contains("explicit validation program refusal")
    );
    assert_eq!(source.0.get(), 0);
}
#[test]
#[ignore = "requires native FFmpeg; strict decoded picture/PCM and frame-clock comparison"]
fn native_media_parity_detects_content_color_clock_and_length_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    ffmpeg(
        root,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=32x32:r=24:d=1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuv444p",
            "-colorspace",
            "bt709",
            "-c:a",
            "pcm_s24le",
            "-ac",
            "2",
            "original.mkv",
        ],
    );
    ffmpeg(
        root,
        &[
            "-i",
            "original.mkv",
            "-c",
            "copy",
            "-metadata",
            "comment=New container same content",
            "rewrapped.mkv",
        ],
    );
    let original = reference(root, "original.mkv");
    let wrapped = reference(root, "rewrapped.mkv");
    assert_ne!(original.sha256, wrapped.sha256);
    let shape = Shape {
        width: 32,
        height: 32,
        fps_numerator: 24,
        fps_denominator: 1,
        frames: 24,
        sample_rate: 48000,
        samples: 48000,
    };
    let compare = |candidate: &str| {
        compare_with_inputs(&original, root, &reference(root, candidate), root, &shape)
    };
    let report = compare("rewrapped.mkv").unwrap();
    assert!(report.equal && report.native_clock_checked);
    assert!(!report.approval_transferred);
    ffmpeg(
        root,
        &[
            "-i",
            "original.mkv",
            "-c",
            "copy",
            "-colorspace",
            "bt470bg",
            "retagged.mkv",
        ],
    );
    let report = compare("retagged.mkv").unwrap();
    assert!(report.picture_planes_equal && report.pcm_equal);
    assert!(!report.displayed_rgb_equal && !report.equal);
    ffmpeg(
        root,
        &[
            "-i",
            "original.mkv",
            "-c",
            "copy",
            "-color_trc",
            "gamma22",
            "changed-transfer.mkv",
        ],
    );
    let report = compare("changed-transfer.mkv").unwrap();
    assert_eq!(
        report.reference.rgb24_frame_sequence_sha256,
        report.candidate.rgb24_frame_sequence_sha256
    );
    assert!(!report.color_metadata_equal && !report.displayed_rgb_equal && !report.equal);
    ffmpeg(
        root,
        &[
            "-i",
            "original.mkv",
            "-vf",
            "negate",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuv444p",
            "-c:a",
            "copy",
            "changed-picture.mkv",
        ],
    );
    let report = compare("changed-picture.mkv").unwrap();
    assert!(!report.picture_planes_equal && !report.equal);
    assert!(report.pcm_equal);
    ffmpeg(
        root,
        &[
            "-i",
            "original.mkv",
            "-c:v",
            "copy",
            "-af",
            "volume=0.5",
            "-c:a",
            "pcm_s24le",
            "changed-audio.mkv",
        ],
    );
    let report = compare("changed-audio.mkv").unwrap();
    assert!(report.picture_planes_equal && report.displayed_rgb_equal);
    assert!(!report.pcm_equal && !report.equal);
    ffmpeg(
        root,
        &["-i", "original.mkv", "-t", "0.5", "-c", "copy", "short.mkv"],
    );
    assert!(compare("short.mkv").is_err());
    ffmpeg(
        root,
        &[
            "-i",
            "original.mkv",
            "-vf",
            "setpts=if(lt(N\\,12)\\,N\\,N+6)/(24*TB)",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuv444p",
            "-c:a",
            "copy",
            "clock-gap.mkv",
        ],
    );
    assert!(
        compare("clock-gap.mkv")
            .unwrap_err()
            .to_string()
            .contains("native clock")
    );
    ffmpeg(
        root,
        &[
            "-i",
            "original.mkv",
            "-c:v",
            "copy",
            "-af",
            "asetpts=PTS+if(gte(N\\,24000)\\,12000\\,0)",
            "-c:a",
            "pcm_s24le",
            "audio-clock-gap.mkv",
        ],
    );
    assert!(
        compare("audio-clock-gap.mkv")
            .unwrap_err()
            .to_string()
            .contains("sample clock")
    );
    let mut stale = wrapped.clone();
    stale.sha256 = "0".repeat(64);
    assert!(compare_with_inputs(&original, root, &stale, root, &shape).is_err());
    let mut wrong_rate = shape.clone();
    wrong_rate.sample_rate = 44100;
    assert!(compare_with_inputs(&original, root, &wrapped, root, &wrong_rate).is_err());
    ffmpeg(
        root,
        &[
            "-i",
            "original.mkv",
            "-c:v",
            "copy",
            "-c:a",
            "pcm_s32le",
            "higher-depth.mkv",
        ],
    );
    assert!(
        compare("higher-depth.mkv")
            .unwrap_err()
            .to_string()
            .contains("integer audio")
    );
}
