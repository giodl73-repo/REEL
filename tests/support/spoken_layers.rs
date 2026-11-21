//! Real bounded FFmpeg with controlled public signals, never production assets.
use super::*;
use reel::{
    cue_relative::{CueMarker, MarkerKind},
    media_runtime::ByteIdentity,
    scene_delivery::{FileRef, InputFiles},
    spoken_programme::layers as api,
};
use std::{path::Path, sync::OnceLock, time::Duration};

struct LayerFiles {
    root: PathBuf,
    runtime: PathBuf,
}
impl InputFiles for LayerFiles {
    fn verified_path(&self, file: &FileRef) -> Result<PathBuf> {
        if file.path == Path::new("runtime/ffmpeg") {
            Ok(self.runtime.clone())
        } else {
            self.root.verified_path(file)
        }
    }
}
fn runtime() -> (PathBuf, &'static api::Runtime) {
    static PIN: OnceLock<(PathBuf, api::Runtime)> = OnceLock::new();
    let value = PIN.get_or_init(|| {
        let path = std::env::var_os("REEL_TEST_FFMPEG")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                    .map(|p| {
                        p.join(if cfg!(windows) {
                            "ffmpeg.exe"
                        } else {
                            "ffmpeg"
                        })
                    })
                    .find(|p| p.is_file())
            })
            .expect("M07 real media qualification requires explicit/pinned FFmpeg; no skip")
            .canonicalize()
            .unwrap();
        let identity = ByteIdentity::observe_file(&path).unwrap();
        let reference = FileRef {
            path: "runtime/ffmpeg".into(),
            sha256: identity.sha256,
            bytes: identity.bytes,
        };
        let files = LayerFiles {
            root: PathBuf::new(),
            runtime: path.clone(),
        };
        let runtime = api::bind_runtime(&files, &reference).unwrap();
        (path, runtime)
    });
    (value.0.clone(), &value.1)
}
fn pin_file(root: &Path, name: &str, bytes: &[u8]) -> FileRef {
    fs::write(root.join(name), bytes).unwrap();
    FileRef {
        path: name.into(),
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
    }
}
fn fixture() -> (Fixture, LayerFiles, &'static api::Runtime) {
    let (runtime, pin) = runtime();
    let mut f = Fixture::new();
    for p in &mut f.document.performances {
        if p.cue_id == "opening" {
            p.markers.push(CueMarker {
                id: "duck".into(),
                kind: MarkerKind::Phrase,
                start_sample: if p.language == "en" { 2400 } else { 4800 },
                end_sample: p.duration_samples - 4800,
            });
        } else if p.cue_id == "closing" {
            let start = if p.language == "en" { 4800 } else { 9600 };
            p.markers.push(CueMarker {
                id: "emphasis".into(),
                kind: MarkerKind::Phrase,
                start_sample: start,
                end_sample: start + 4800,
            });
        }
    }
    f.sync_metadata();
    f.produce(&ProcessingPolicy::identity()).unwrap();
    let root = f.temp.path().join("layers");
    fs::create_dir(&root).unwrap();
    let bed: Vec<_> = (0..216000)
        .flat_map(|i| [7000 + i % 31, 3000 - i % 19])
        .collect();
    let effect: Vec<_> = (0..4800)
        .flat_map(|i| [15000 + i % 17, -8000 + i % 13])
        .collect();
    pin_file(&root, "bed.wav", &wave(&bed));
    pin_file(&root, "effect.wav", &wave(&effect));
    (f, LayerFiles { root, runtime }, pin)
}
fn request(f: &Fixture, files: &LayerFiles, edition: &str) -> api::Request {
    let receipt: producer::ProductionReceipt =
        serde_json::from_slice(&fs::read(f.output().join("receipt.json")).unwrap()).unwrap();
    let parent = receipt
        .editions
        .iter()
        .find(|e| e.edition_id == edition)
        .unwrap();
    let lang = edition.split('-').next().unwrap();
    let anchor = |cue: &str, marker: &str, edge| {
        let p = f
            .document
            .performances
            .iter()
            .find(|p| p.id == format!("{lang}-{cue}"))
            .unwrap();
        api::Anchor {
            performance_id: p.id.clone(),
            source_sha256: p.source.sha256.clone(),
            marker_id: marker.into(),
            edge,
        }
    };
    let blank = FileRef {
        path: "pending".into(),
        sha256: "0".repeat(64),
        bytes: 1,
    };
    let mut request = api::Request {
        schema: api::SCHEMA.into(),
        edition_id: edition.into(),
        compiled_recipe_sha256: parent.compiled_recipe_sha256.clone(),
        execution_recipe_sha256: parent.execution_recipe_sha256.clone(),
        sonic_catalog: blank.clone(),
        sonic_request: blank,
        intent: api::Intent {
            bed_gain_db: -12.0,
            duck_gain_db: -6.0,
            effect_gain_db: -6.0,
            fade_samples: 2400,
            duck_start: anchor("opening", "duck", api::Edge::Start),
            duck_end: anchor("opening", "duck", api::Edge::End),
            effect_start: anchor("closing", "emphasis", api::Edge::Start),
            allow_dialogue_overlap: true,
            allow_layer_overlap: true,
        },
    };
    let assets = ["bed","effect"].into_iter().map(|name| {
        let bytes = fs::read(files.root.join(format!("{name}.wav"))).unwrap();
        json!({"asset_id":name,"authority_state":"fixture-only","authority_receipt_sha256":null,
            "license":{"license_id":"synthetic-fixture","review_status":"engineering-only","permits_production_use":false},
            "variants":[{"variant_id":"v1","locator":format!("{name}.wav"),"sha256":hash(&bytes),"bytes":bytes.len(),
                "geometry":{"sample_rate_hz":48000,"bit_depth":24,"channels":2,"sample_count":if name=="bed"{216000}else{4800}}}]})
    }).collect::<Vec<_>>();
    request.sonic_catalog = pin_file(&files.root, &format!("catalog-{edition}.json"), &serde_json::to_vec(&json!({
        "schema":reel::sonic_assets::CATALOG_SCHEMA,"library_id":"spoken-fixture","library_version":"1","assets":assets
    })).unwrap());
    sync_request(files, &mut request);
    request
}
fn sync_request(files: &LayerFiles, request: &mut api::Request) {
    request.sonic_request = pin_file(&files.root, &format!("request-{}.json",request.edition_id), &serde_json::to_vec(&json!({
        "schema":reel::sonic_assets::REQUEST_SCHEMA,"request_id":"spoken-fixture","consumer_manifest_sha256":api::consumer_sha256(request).unwrap(),
        "engineering_fixture":true,"bindings":[
            {"event_id":"bed","selection":"exact","asset_id":"bed","variant_id":"v1"},
            {"event_id":"effect","selection":"exact","asset_id":"effect","variant_id":"v1"}]
    })).unwrap());
}
#[test]
#[ignore = "requires pinned FFmpeg; explicitly qualified on both CI media lanes"]
fn real_four_edition_layers_follow_native_clocks_and_independent_pcm() {
    let (f, files, runtime) = fixture();
    let source_root = f.temp.path().to_path_buf();
    let dialogue = f.output();
    let policy = ProcessingPolicy::identity();
    for (edition, start) in [
        ("en-executive", 160800),
        ("en-technical", 369600),
        ("es-executive", 194400),
        ("es-technical", 432000),
    ] {
        let request = request(&f, &files, edition);
        let prepared = api::prepare(
            api::Inputs {
                document: &f.document,
                processing: &policy,
                sources: &source_root,
                dialogue_candidate: &dialogue,
                layers: &files,
            },
            &request,
            runtime,
        )
        .unwrap();
        assert_eq!(prepared.effect_start_sample(), start);
        let output = f.temp.path().join(format!("mix-{edition}"));
        let receipt = api::produce(&prepared, &output, Duration::from_secs(30), || Ok(())).unwrap();
        assert_eq!(receipt.effect_start_sample, start);
        let checked = api::check(&prepared, &output, Duration::from_secs(30)).unwrap();
        assert!(
            checked.independent_pcm_check_passed
                && !checked.authority_verified
                && !checked.release_authorized
        );
        // Independent test probe, beyond the production checker's complete oracle.
        // Use a constant-frame point inside the duck, away from fades.
        let music = read_pcm(&output.join("music.wav"));
        let source = read_pcm(&files.root.join("bed.wav"));
        let sample = 12000usize * 2;
        let expected = source[sample] as f64 * 10f64.powf(-18.0 / 20.0);
        assert!((music[sample] as f64 - expected).abs() <= 2.0);
        assert_eq!(
            read_pcm(&output.join("dialogue.wav")),
            read_pcm(&dialogue.join(&prepared.dialogue().path))
        );
    }
}
fn read_pcm(path: &Path) -> Vec<i32> {
    let raw = fs::read(path).unwrap();
    // Test-owned RIFF chunk walk; FFmpeg's WAVE header need not be 44 bytes.
    let mut at = 12;
    loop {
        let count = u32::from_le_bytes(raw[at + 4..at + 8].try_into().unwrap()) as usize;
        if &raw[at..at + 4] == b"data" {
            return raw[at + 8..at + 8 + count]
                .chunks_exact(3)
                .map(|b| {
                    i32::from_le_bytes([b[0], b[1], b[2], if b[2] & 128 == 0 { 0 } else { 255 }])
                })
                .collect();
        }
        at += 8 + count + (count % 2);
    }
}

#[test]
#[ignore = "requires pinned FFmpeg; explicitly qualified on both CI media lanes"]
fn revised_bed_changes_mix_without_changing_dialogue_and_tamper_refuses() {
    let (f, files, runtime) = fixture();
    let dialogue = f.output();
    let policy = ProcessingPolicy::identity();
    let before = request(&f, &files, "en-executive");
    let source_root = f.temp.path().to_path_buf();
    let inputs = api::Inputs {
        document: &f.document,
        processing: &policy,
        sources: &source_root,
        dialogue_candidate: &dialogue,
        layers: &files,
    };
    let old = api::prepare(inputs, &before, runtime).unwrap();
    let old_identity = old.identity().to_owned();
    let old_dialogue = old.dialogue().clone();
    let original_output = f.temp.path().join("v1");
    api::produce(&old, &original_output, Duration::from_secs(30), || Ok(())).unwrap();
    let bytes = fs::read(files.root.join("bed.wav")).unwrap();
    let mut pcm = decode(&bytes);
    pcm[5000] += 1000;
    // Separate v2 file: original source/version remains intact.
    pin_file(&files.root, "bed-v2.wav", &wave(&pcm));
    let mut catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(files.root.join(&before.sonic_catalog.path)).unwrap())
            .unwrap();
    let bytes = fs::read(files.root.join("bed-v2.wav")).unwrap();
    catalog["assets"][0]["variants"][0]["locator"] = json!("bed-v2.wav");
    catalog["assets"][0]["variants"][0]["sha256"] = json!(hash(&bytes));
    let mut after = before.clone();
    after.sonic_catalog = pin_file(
        &files.root,
        "catalog-v2.json",
        &serde_json::to_vec(&catalog).unwrap(),
    );
    let new = api::prepare(inputs, &after, runtime).unwrap();
    assert_ne!(old_identity, new.identity());
    assert_eq!(
        serde_json::to_value(old_dialogue).unwrap(),
        serde_json::to_value(new.dialogue()).unwrap()
    );
    assert_eq!(before.compiled_recipe_sha256, after.compiled_recipe_sha256);
    let output = f.temp.path().join("v2");
    api::produce(&new, &output, Duration::from_secs(30), || Ok(())).unwrap();
    assert_ne!(
        fs::read(original_output.join("mix.wav")).unwrap(),
        fs::read(output.join("mix.wav")).unwrap()
    );
    assert_eq!(
        fs::read(original_output.join("dialogue.wav")).unwrap(),
        fs::read(output.join("dialogue.wav")).unwrap()
    );
    assert!(api::check(&old, &output, Duration::from_secs(30)).is_err());
    let mut raw = fs::read(output.join("mix.wav")).unwrap();
    let last = raw.len() - 6;
    raw[last..last + 3].copy_from_slice(&100000i32.to_le_bytes()[..3]);
    fs::write(output.join("mix.wav"), &raw).unwrap();
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("receipt.json")).unwrap()).unwrap();
    receipt["outputs"]["mix.wav"]["sha256"] = json!(hash(&raw));
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    assert!(api::check(&new, &output, Duration::from_secs(30)).is_err());
    assert!(files.root.join("bed.wav").is_file());
}

#[test]
#[ignore = "requires pinned FFmpeg; explicitly qualified on both CI media lanes"]
fn finer_anchor_and_full_effect_tail_overrun_refuse_without_rounding() {
    let (mut f, files, runtime) = fixture();
    let original = f.document.clone();
    let source_root = f.temp.path().to_path_buf();
    let policy = ProcessingPolicy::identity();
    for (case, diagnostic) in [(0, "millisecond clock"), (1, "tail/window")] {
        f.document = original.clone();
        let p = f
            .document
            .performances
            .iter_mut()
            .find(|p| p.id == "en-closing")
            .unwrap();
        p.markers[0].start_sample = if case == 0 {
            4801
        } else {
            p.duration_samples - 2400
        };
        p.markers[0].end_sample = if case == 0 { 9600 } else { p.duration_samples };
        f.sync_metadata();
        let dialogue = f.temp.path().join(format!("parent-{case}"));
        let receipt = producer::produce(
            &f.document,
            &f.plan(),
            &policy,
            &source_root,
            &dialogue,
            || Ok(()),
        )
        .unwrap();
        let parent = receipt
            .editions
            .iter()
            .find(|e| e.edition_id == "en-executive")
            .unwrap();
        let mut request = request(&f, &files, "en-executive");
        request.compiled_recipe_sha256 = parent.compiled_recipe_sha256.clone();
        request.execution_recipe_sha256 = parent.execution_recipe_sha256.clone();
        sync_request(&files, &mut request);
        let result = api::prepare(
            api::Inputs {
                document: &f.document,
                processing: &policy,
                sources: &source_root,
                dialogue_candidate: &dialogue,
                layers: &files,
            },
            &request,
            runtime,
        );
        let error = match result {
            Ok(_) => panic!("invalid clock/tail admitted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains(diagnostic), "{error:#}");
        assert!(!f.temp.path().join("layer-output").exists());
    }
}

#[test]
#[ignore = "requires pinned FFmpeg; explicitly qualified on both CI media lanes"]
fn stale_scope_overlap_gain_tail_and_clock_refuse_before_writes() {
    let (f, files, runtime) = fixture();
    let dialogue = f.output();
    let policy = ProcessingPolicy::identity();
    let baseline = request(&f, &files, "en-executive");
    let source_root = f.temp.path().to_path_buf();
    let inputs = api::Inputs {
        document: &f.document,
        processing: &policy,
        sources: &source_root,
        dialogue_candidate: &dialogue,
        layers: &files,
    };
    for mutation in 0..7 {
        let mut bad = baseline.clone();
        match mutation {
            0 => bad.intent.effect_start.source_sha256 = "1".repeat(64),
            1 => bad.intent.effect_start.marker_id = "missing".into(),
            2 => bad.intent.allow_dialogue_overlap = false,
            3 => bad.intent.allow_layer_overlap = false,
            4 => bad.intent.effect_gain_db = 1.0,
            5 => bad.intent.fade_samples = 2401,
            _ => bad.intent.effect_start.performance_id = "es-closing".into(),
        }
        sync_request(&files, &mut bad);
        assert!(api::prepare(inputs, &bad, runtime).is_err());
    }
    let mut no_scope = baseline.clone();
    let mut catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(files.root.join(&baseline.sonic_catalog.path)).unwrap())
            .unwrap();
    catalog["assets"][0]["authority_state"] = json!("candidate");
    no_scope.sonic_catalog = pin_file(
        &files.root,
        "unscoped.json",
        &serde_json::to_vec(&catalog).unwrap(),
    );
    sync_request(&files, &mut no_scope);
    assert!(api::prepare(inputs, &no_scope, runtime).is_err());
    // Restore exact consumer request after negative mutations.
    let mut baseline = baseline;
    sync_request(&files, &mut baseline);
    let valid = api::prepare(inputs, &baseline, runtime).unwrap();
    let output = f.temp.path().join("refused");
    assert!(api::produce(&valid, &output, Duration::ZERO, || Ok(())).is_err());
    assert!(!output.exists());
    assert!(
        api::produce(&valid, &output, Duration::from_secs(30), || bail!(
            "injected interruption"
        ))
        .is_err()
    );
    assert!(!output.exists());
    fs::create_dir(&output).unwrap();
    fs::write(output.join("foreign"), b"keep").unwrap();
    assert!(api::produce(&valid, &output, Duration::from_secs(30), || Ok(())).is_err());
    assert_eq!(fs::read(output.join("foreign")).unwrap(), b"keep");
}
