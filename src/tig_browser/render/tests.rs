use super::*;
use crate::{
    browser_composition::{BrowserCompositionManifest, HashedAsset},
    media_runtime::{Bindings, ByteIdentity},
    scene_delivery::FileRef,
};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    path::PathBuf,
};

struct Files {
    paths: BTreeMap<PathBuf, (FileRef, PathBuf, File)>,
}
impl InputFiles for Files {
    fn verified_path(&self, file: &FileRef) -> Result<PathBuf> {
        let (expected, path, _) = self
            .paths
            .get(&file.path)
            .context("fixture file not bound")?;
        if expected != file {
            bail!("fixture pin differs");
        }
        Ok(path.clone())
    }
}
fn pin(path: &Path) -> FileRef {
    let identity = ByteIdentity::observe_file(path).unwrap();
    FileRef {
        path: path.to_owned(),
        sha256: identity.sha256,
        bytes: identity.bytes,
    }
}
fn runtime() -> &'static (BoundPrograms, BoundTool) {
    static RUNTIME: std::sync::OnceLock<(BoundPrograms, BoundTool)> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        let browser = pin(Path::new(
            &std::env::var("REEL_TEST_BROWSER").expect("explicit REEL_TEST_BROWSER required"),
        ));
        let bindings = Bindings {
            host: ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap(),
            ffmpeg: pin(Path::new(
                &std::env::var("REEL_TEST_FFMPEG").expect("explicit REEL_TEST_FFMPEG required"),
            )),
            ffprobe: pin(Path::new(
                &std::env::var("REEL_TEST_FFPROBE").expect("explicit REEL_TEST_FFPROBE required"),
            )),
            dependencies: BTreeMap::from([("browser/program".into(), browser.clone())]),
        };
        (
            BoundPrograms::bind(&bindings).unwrap(),
            BoundTool::bind(&browser).unwrap(),
        )
    })
}
fn wave() -> Vec<u8> {
    let data = [0u8, 0, 16, 0, 0, 248].repeat(12000);
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(288000u32.to_le_bytes());
    wav.extend(6u16.to_le_bytes());
    wav.extend(24u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    wav
}
fn fixture(root: &Path, audio: bool, corrupt: bool) -> (Prepared, Files) {
    use std::os::windows::fs::OpenOptionsExt;
    let svg=r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><image href="pictures/niño 1%+&amp;.svg" width="8" height="8"/><rect id="r" width="8" height="8" fill="red"/><script>addEventListener('reelframe',e=>document.getElementById('r').setAttribute('fill',e.detail.frame%2?'blue':'red'))</script></svg>"##.as_bytes();
    let mut inputs = BTreeMap::new();
    let mut paths = BTreeMap::new();
    let mut total = 0;
    for (name, id, bytes) in std::iter::once(("opening.svg", "svg/main", svg.to_vec()))
        .chain(std::iter::once(("pictures/niño 1%+&.svg","image/complex",br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8"/></svg>"#.to_vec())))
        .chain(audio.then(|| ("audio.wav", "audio/main", wave())))
    {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            if corrupt && name == "opening.svg" {
                b"corrupt".as_slice()
            } else {
                &bytes
            },
        )
        .unwrap();
        let object = Object::from_bytes(&bytes);
        let file = FileRef {
            path: id.into(),
            sha256: object.sha256.as_str().into(),
            bytes: object.bytes,
        };
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        total += file.bytes;
        inputs.insert(name.to_owned(), file.clone());
        paths.insert(file.path.clone(), (file, path, held));
    }
    let source = HashedAsset {
        path: "opening.svg".into(),
        sha256: inputs["opening.svg"].sha256.clone(),
    };
    let manifest = BrowserCompositionManifest {
        schema: crate::browser_composition::MANIFEST_SCHEMA.into(),
        source,
        assets: vec![HashedAsset {
            path: "pictures/niño 1%+&.svg".into(),
            sha256: inputs["pictures/niño 1%+&.svg"].sha256.clone(),
        }],
        width: 8,
        height: 8,
        fps: 24,
        frame_count: 6,
        dynamic_regions: vec![],
        audio: audio.then(|| HashedAsset {
            path: "audio.wav".into(),
            sha256: inputs["audio.wav"].sha256.clone(),
        }),
    };
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
    (
        Prepared {
            source_checkpoint: tig_core::Digest::blob(b"synthetic source"),
            source_record: tig_assets::graph::Reference {
                id: "browser/source".into(),
                object: Object::from_bytes(b"synthetic record"),
            },
            manifest,
            manifest_bytes,
            inputs,
            alias_bytes: total,
            unique_bytes: total,
            unique_objects: if audio { 3 } else { 2 },
        },
        Files { paths },
    )
}
fn limits() -> Limits {
    Limits {
        capture: capture::Limits {
            max_requests: 32,
            max_resource_bytes: 1024 * 1024,
            max_served_bytes: 4 * 1024 * 1024,
            max_message_bytes: 4 * 1024 * 1024,
            max_messages: 4096,
            max_png_bytes: 1024 * 1024,
            max_frames: 6,
            max_canvas_pixels: 64,
            max_elapsed_ms: 60000,
            max_io_wait_ms: 15000,
        },
        max_workspace_bytes: 128 * 1024 * 1024,
        max_output_bytes: 1024 * 1024,
        browser_memory_bytes: 2 * 1024 * 1024 * 1024,
        browser_processes: 64,
        encoder_memory_bytes: 256 * 1024 * 1024,
        encoder_processes: 4,
    }
}
#[test]
#[ignore = "requires explicit enrolled local browser/FFmpeg; synthetic media only"]
fn real_native_browser_streams_svg_and_exact_audio_without_spools() {
    let root = tempfile::tempdir().unwrap();
    let (programs, browser) = runtime();
    let (plan, inputs) = fixture(root.path(), true, false);
    let output = root.path().join("master.mkv");
    let result = native(
        &plan,
        &inputs,
        programs,
        browser,
        root.path(),
        &output,
        &limits(),
        true,
        || Ok(()),
    )
    .unwrap();
    assert_eq!(result.frames, 6);
    assert_eq!(
        result.capture.served_bytes,
        plan.inputs["opening.svg"].bytes + plan.inputs["pictures/niño 1%+&.svg"].bytes
    );
    assert_eq!(result.samples_per_channel, Some(12000));
    assert_eq!(
        result.pcm_sha256,
        Some(crate::production::sha256_bytes(&wave()[44..]))
    );
    assert_eq!(result.output.bytes, fs::metadata(&output).unwrap().len());
    let frames = root.path().join("frames.md5");
    let args = vec![
        "-v".into(),
        "error".into(),
        "-nostdin".into(),
        "-i".into(),
        native_path(&output).unwrap(),
        "-map".into(),
        "0:v:0".into(),
        "-f".into(),
        "framemd5".into(),
        native_path(root.path()).unwrap() + "/frames.md5",
    ];
    let mut child = Task::start(
        &programs
            .with_inputs(&inputs)
            .media_program(MediaProgram::Ffmpeg)
            .unwrap(),
        args,
        root.path(),
        128 * 1024 * 1024,
        Duration::from_secs(10),
        limits().encoder(),
    )
    .unwrap();
    child
        .wait(Instant::now() + Duration::from_secs(10))
        .unwrap();
    let text = fs::read_to_string(&frames).unwrap();
    assert!(text.len() < 4096);
    let rows = text
        .lines()
        .filter(|s| !s.starts_with('#'))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 6);
    assert_ne!(
        rows[0].rsplit(',').next(),
        rows[1].rsplit(',').next(),
        "source frame clock did not change the picture"
    );
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        5,
        "unexpected profile, staging or frame files remained"
    );
}
#[test]
#[ignore = "requires explicit enrolled local browser/FFmpeg; synthetic media only"]
fn real_native_browser_corrupt_source_cleans_owned_children_and_preserves_other_files() {
    let root = tempfile::tempdir().unwrap();
    let (programs, browser) = runtime();
    let (plan, inputs) = fixture(root.path(), false, true);
    let output = root.path().join("master.mkv");
    fs::write(root.path().join("keep.mkv"), b"foreign retained output").unwrap();
    let error = native(
        &plan,
        &inputs,
        programs,
        browser,
        root.path(),
        &output,
        &limits(),
        false,
        || Ok(()),
    )
    .err()
    .expect("corrupt source accepted");
    eprintln!("synthetic native refusal: {error:#}");
    assert!(
        format!("{error:#}").contains("browser resource differs from admitted bytes"),
        "unexpected early refusal: {error:#}"
    );
    assert!(!output.exists());
    assert_eq!(
        fs::read(root.path().join("keep.mkv")).unwrap(),
        b"foreign retained output"
    );
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        3,
        "failed operation left entries: {:?}",
        fs::read_dir(root.path())
            .unwrap()
            .map(|v| v.unwrap().file_name())
            .collect::<Vec<_>>()
    );
}
