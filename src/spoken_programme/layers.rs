//! Authored spoken-edition layers over the existing Sonic resolver and D/M/E mixer.
use super::{checker, compiler, producer, *};
use crate::{
    audio_mix,
    production::{AudioEvent, AudioRole, GainAutomationPoint, GainCurve, ProductionManifest},
    scene_delivery::{FileRef, InputFiles},
    sonic_assets,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

pub const SCHEMA: &str = "reel.spoken-layers.v1";
pub const RECEIPT_SCHEMA: &str = "reel.spoken-layer-receipt.v1";
const WAV_MAX: u64 = 16 * 1024 * 1024;
const OUTPUT_NAMES: [&str; 4] = ["dialogue.wav", "music.wav", "effects.wav", "mix.wav"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub performance_id: String,
    pub source_sha256: String,
    pub marker_id: String,
    pub edge: Edge,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Edge {
    Start,
    End,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub bed_gain_db: f64,
    pub duck_gain_db: f64,
    pub duck_start: Anchor,
    pub duck_end: Anchor,
    pub effect_start: Anchor,
    pub effect_gain_db: f64,
    pub fade_samples: u64,
    pub allow_dialogue_overlap: bool,
    pub allow_layer_overlap: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub edition_id: String,
    pub compiled_recipe_sha256: String,
    pub execution_recipe_sha256: String,
    pub intent: Intent,
    pub sonic_catalog: FileRef,
    pub sonic_request: FileRef,
}
fn digest(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
/// Existing Sonic consumer pin: exact edition intent, without circular pointers.
pub fn consumer_sha256(request: &Request) -> Result<String> {
    Ok(digest(&serde_json::to_vec(&(
        SCHEMA,
        &request.edition_id,
        &request.compiled_recipe_sha256,
        &request.execution_recipe_sha256,
        &request.intent,
    ))?))
}

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub document: &'a Document,
    pub processing: &'a producer::ProcessingPolicy,
    pub sources: &'a dyn InputFiles,
    pub dialogue_candidate: &'a Path,
    pub layers: &'a dyn InputFiles,
}
struct Source {
    reference: FileRef,
    path: PathBuf,
    pcm: Vec<u8>,
}
/// Sealed control/source interpretation. Borrowed providers retain their guards.
pub struct Prepared<'a> {
    inputs: Inputs<'a>,
    request: Request,
    runtime: &'a Runtime,
    sources: Vec<Source>,
    samples: u64,
    effect_start: u64,
    duck_start: u64,
    duck_end: u64,
    graph: audio_mix::CompiledAudioMix,
    identity: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub mix_identity_sha256: String,
    pub request: Request,
    pub runtime: FileRef,
    pub dialogue: FileRef,
    pub samples: u64,
    pub effect_start_sample: u64,
    pub duck_start_sample: u64,
    pub duck_end_sample: u64,
    pub outputs: BTreeMap<String, FileRef>,
    pub authority_verified: bool,
    pub release_authorized: bool,
}
#[derive(Debug, Serialize)]
pub struct CheckSummary {
    pub independent_pcm_check_passed: bool,
    pub authority_verified: bool,
    pub release_authorized: bool,
}
impl Prepared<'_> {
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn effect_start_sample(&self) -> u64 {
        self.effect_start
    }
    pub fn dialogue(&self) -> &FileRef {
        &self.sources[0].reference
    }
}
fn checked_control<T: serde::de::DeserializeOwned>(
    inputs: &dyn InputFiles,
    file: &FileRef,
) -> Result<T> {
    require(
        file.bytes <= MAX_DECLARATION_BYTES as u64,
        "spoken layer control budget exceeded",
    )?;
    let raw = producer::read_source(inputs, file, &mut || Ok(()))?;
    serde_json::from_slice(&raw).map_err(|_| anyhow::anyhow!("spoken layer control JSON invalid"))
}
/// Existing native executable guard on Windows; explicit cooperative rechecks
/// on other platforms. No downloaded tool or complete-environment claim.
pub struct Runtime {
    reference: FileRef,
    path: PathBuf,
    protection: Option<crate::media_runtime::BoundTool>,
}
pub fn bind_runtime(inputs: &dyn InputFiles, file: &FileRef) -> Result<Runtime> {
    require(
        hash(&file.sha256) && file.bytes > 0 && file.bytes <= 256 * 1024 * 1024,
        "spoken layer runtime budget/pin invalid",
    )?;
    let path = inputs
        .verified_path(file)
        .map_err(|_| anyhow::anyhow!("spoken layer runtime unavailable"))?;
    require(
        path.is_absolute()
            && !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")),
        "spoken layer runtime must be an explicit executable",
    )?;
    require(
        path.as_os_str().len() <= 1024,
        "spoken layer runtime path budget exceeded",
    )?;
    let guard = if cfg!(windows) {
        Some(
            crate::media_runtime::BoundTool::bind(&FileRef {
                path: path.clone(),
                sha256: file.sha256.clone(),
                bytes: file.bytes,
            })
            .map_err(|_| anyhow::anyhow!("spoken layer runtime protection/pin refused"))?,
        )
    } else {
        None
    };
    let value = Runtime {
        reference: file.clone(),
        path,
        protection: guard,
    };
    value.verify()?;
    Ok(value)
}
impl Runtime {
    fn verify(&self) -> Result<()> {
        if self.protection.is_none() {
            let identity = crate::media_runtime::ByteIdentity::observe_file_bounded(
                &self.path,
                self.reference.bytes,
            )
            .map_err(|_| anyhow::anyhow!("spoken layer cooperative runtime unavailable"))?;
            require(
                identity.sha256 == self.reference.sha256 && identity.bytes == self.reference.bytes,
                "spoken layer runtime differs",
            )?;
        }
        Ok(())
    }
}
fn source(inputs: &(impl InputFiles + ?Sized), file: FileRef) -> Result<Source> {
    require(file.bytes <= WAV_MAX, "spoken layer source budget exceeded")?;
    let raw = producer::read_source(inputs, &file, &mut || Ok(()))?;
    let pcm = crate::song_edition::wave::pcm24(&raw)
        .map_err(|_| anyhow::anyhow!("spoken layer PCM format unsupported"))?
        .to_vec();
    let path = inputs.verified_path(&file)?;
    Ok(Source {
        reference: file,
        path,
        pcm,
    })
}
fn anchor(edition: &compiler::CompiledEdition, anchor: &Anchor) -> Result<u64> {
    let cue = edition
        .recipe
        .cues
        .iter()
        .find(|c| c.performance.id == anchor.performance_id)
        .context("spoken layer selected anchor missing")?;
    require(
        cue.performance.source.sha256 == anchor.source_sha256,
        "spoken layer anchor source stale",
    )?;
    let marker = edition
        .markers
        .iter()
        .find(|m| m.performance_id == anchor.performance_id && m.marker.id == anchor.marker_id)
        .context("spoken layer marker missing")?;
    let sample = match anchor.edge {
        Edge::Start => marker.marker.start_sample,
        Edge::End => marker.marker.end_sample,
    };
    require(
        sample <= edition.duration_samples && sample % 48 == 0,
        "spoken layer anchor outside millisecond clock",
    )?;
    Ok(sample)
}
fn event(id: &str, role: AudioRole, start: u64, samples: u64, gain: f64, fade: u64) -> AudioEvent {
    AudioEvent {
        id: id.into(),
        role,
        source: String::new(),
        start_seconds: start as f64 / 48000.0,
        duration_seconds: Some(samples as f64 / 48000.0),
        source_in_seconds: 0.0,
        gain_db: gain,
        loop_source: false,
        fade_in_ms: fade / 48,
        fade_out_ms: fade / 48,
        beat_marker_id: None,
        gain_automation: vec![],
    }
}
fn point(sample: u64, gain_db: f64) -> GainAutomationPoint {
    GainAutomationPoint {
        time_seconds: Some(sample as f64 / 48000.0),
        beat_marker_id: None,
        gain_db,
        curve: GainCurve::Hold,
    }
}

pub fn prepare<'a>(
    inputs: Inputs<'a>,
    request: &Request,
    runtime: &'a Runtime,
) -> Result<Prepared<'a>> {
    require(
        request.schema == SCHEMA
            && id(&request.edition_id)
            && hash(&request.compiled_recipe_sha256)
            && hash(&request.execution_recipe_sha256),
        "spoken layer request invalid",
    )?;
    let intent = &request.intent;
    require(
        [
            intent.bed_gain_db,
            intent.duck_gain_db,
            intent.effect_gain_db,
        ]
        .iter()
        .all(|v| {
            v.is_finite()
                && (-60.0..=0.0).contains(v)
                && (v * 1000.0 - (v * 1000.0).round()).abs() <= 1e-8
        }) && intent.fade_samples <= 48000
            && intent.fade_samples % 48 == 0,
        "spoken layer gain/fade invalid",
    )?;
    let producer_runtime = producer::current_runtime()?;
    let receipt = producer::bounded(
        &inputs.dialogue_candidate.join("receipt.json"),
        16 * 1024 * 1024,
    )?;
    let parent_receipt: producer::ProductionReceipt = serde_json::from_slice(&receipt)
        .map_err(|_| anyhow::anyhow!("spoken layer parent receipt invalid"))?;
    let plan = compiler::compile_with_inputs(
        inputs.document,
        inputs.sources,
        &producer_runtime,
        parent_receipt.compilation_profile,
    )?;
    let plan_bytes = serde_json::to_vec_pretty(&plan)?;
    let checked = checker::check(
        checker::CheckRequest {
            document: inputs.document,
            plan_bytes: &plan_bytes,
            receipt_bytes: &receipt,
            processing: inputs.processing,
            expected_runtime: &producer_runtime,
        },
        inputs.sources,
        inputs.dialogue_candidate,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )?;
    let access = checked
        .access()
        .editions
        .iter()
        .find(|e| e.edition_id == request.edition_id)
        .context("spoken layer edition missing")?;
    require(
        access.compiled_recipe_sha256 == request.compiled_recipe_sha256
            && access.execution_recipe_sha256 == request.execution_recipe_sha256,
        "spoken layer parent recipe stale",
    )?;
    let edition = plan
        .editions
        .iter()
        .find(|e| e.recipe.edition_id == request.edition_id)
        .context("spoken layer compiled edition missing")?;
    require(
        edition.duration_samples <= 720000 && edition.duration_samples % 48 == 0,
        "spoken layer duration unsupported",
    )?;
    let effect_start = anchor(edition, &intent.effect_start)?;
    let duck_start = anchor(edition, &intent.duck_start)?;
    let duck_end = anchor(edition, &intent.duck_end)?;
    let catalog: sonic_assets::Catalog = checked_control(inputs.layers, &request.sonic_catalog)?;
    require(
        catalog.assets.len() <= 16
            && catalog.assets.iter().all(|a| a.variants.len() <= 16)
            && catalog.pools.len() <= 16,
        "spoken layer catalog count exceeded",
    )?;
    let selection: sonic_assets::Request = checked_control(inputs.layers, &request.sonic_request)?;
    require(
        selection.bindings.len() == 2
            && selection.bindings.iter().all(|b| {
                (b.event_id == "bed" || b.event_id == "effect")
                    && matches!(b.selection, sonic_assets::SelectionMode::Exact)
            })
            && selection.consumer_manifest_sha256 == consumer_sha256(request)?,
        "spoken layer Sonic consumer differs",
    )?;
    let catalog = sonic_assets::Loaded {
        path: PathBuf::new(),
        sha256: request.sonic_catalog.sha256.clone(),
        value: catalog,
    };
    let selection = sonic_assets::Loaded {
        path: PathBuf::new(),
        sha256: request.sonic_request.sha256.clone(),
        value: selection,
    };
    let mut selected_sizes = BTreeMap::from([(access.wave.sha256.clone(), access.wave.bytes)]);
    let (resolved, _) = sonic_assets::resolve_with_paths(&catalog, &selection, |variant| {
        require(
            variant.bytes <= WAV_MAX && variant.geometry.sample_count <= 720000,
            "spoken layer selected source exceeds budget",
        )?;
        selected_sizes.insert(variant.sha256.clone(), variant.bytes);
        require(
            selected_sizes.values().sum::<u64>() <= WAV_MAX,
            "spoken layer aggregate selected budget exceeded",
        )?;
        inputs.layers.verified_path(&FileRef {
            path: variant.locator.clone().into(),
            sha256: variant.sha256.clone(),
            bytes: variant.bytes,
        })
    })
    .map_err(|_| anyhow::anyhow!("spoken layer Sonic selection refused"))?;
    let mut sources = vec![source(inputs.dialogue_candidate, access.wave.clone())?];
    for name in ["bed", "effect"] {
        let value = resolved
            .selections
            .iter()
            .find(|s| s.event_id == name)
            .context("spoken layer selected event missing")?;
        // Use the original admitted locator, never the resolver's returned bare path.
        let variant = catalog
            .value
            .assets
            .iter()
            .find(|a| a.asset_id == value.asset_id)
            .and_then(|a| a.variants.iter().find(|v| v.variant_id == value.variant_id))
            .context("spoken layer variant missing")?;
        sources.push(source(
            inputs.layers,
            FileRef {
                path: variant.locator.clone().into(),
                sha256: value.sha256.clone(),
                bytes: value.bytes,
            },
        )?);
    }
    let mut distinct = BTreeMap::new();
    for source in &sources {
        distinct.insert(&source.reference.sha256, source.reference.bytes);
    }
    require(
        distinct.values().sum::<u64>() <= WAV_MAX,
        "spoken layer aggregate source budget exceeded",
    )?;
    let bed_samples = sources[1].pcm.len() as u64 / 6;
    let effect_samples = sources[2].pcm.len() as u64 / 6;
    require(
        bed_samples > 0
            && effect_samples > 0
            && bed_samples % 48 == 0
            && effect_samples % 48 == 0
            && intent.fade_samples * 2 <= bed_samples.min(effect_samples)
            && bed_samples <= edition.duration_samples
            && effect_start
                .checked_add(effect_samples)
                .is_some_and(|end| end <= edition.duration_samples)
            && 0 < duck_start
            && duck_start < duck_end
            && duck_end < bed_samples,
        "spoken layer full-source tail/window invalid",
    )?;
    let overlaps = |start: u64, end: u64| {
        edition
            .cues
            .iter()
            .any(|c| start < c.end_sample && c.start_sample < end)
    };
    require(
        intent.allow_dialogue_overlap
            || (!overlaps(0, bed_samples)
                && !overlaps(effect_start, effect_start + effect_samples)),
        "spoken layer dialogue overlap undeclared",
    )?;
    require(
        intent.allow_layer_overlap || effect_start >= bed_samples,
        "spoken layer bus overlap undeclared",
    )?;
    let mut bed = event(
        "bed",
        AudioRole::Music,
        0,
        bed_samples,
        intent.bed_gain_db,
        intent.fade_samples,
    );
    bed.gain_automation = vec![
        point(0, 0.0),
        point(duck_start, intent.duck_gain_db),
        point(duck_end, 0.0),
    ];
    let events = vec![
        event(
            "dialogue",
            AudioRole::Dialogue,
            0,
            edition.duration_samples,
            0.0,
            0,
        ),
        bed,
        event(
            "effect",
            AudioRole::Effect,
            effect_start,
            effect_samples,
            intent.effect_gain_db,
            intent.fade_samples,
        ),
    ];
    let projection: ProductionManifest = serde_json::from_value(serde_json::json!({
        "work":request.edition_id, "title":"", "audio_events":events
    }))?;
    let mut graph = audio_mix::compile(
        &projection,
        edition.duration_samples as f64 / 48000.0,
        0,
        true,
        48000,
        2,
    )?;
    // Canonical 1 ms input frames make existing hold automation sample-clock explicit.
    for index in 0..3 {
        let original = format!("[{index}:a:0]");
        let framed = format!("[spoken_layer_{index}]");
        for filter in &mut graph.filters {
            *filter = filter.replace(&original, &framed);
        }
        graph
            .filters
            .insert(index, format!("{original}aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo,asetnsamples=n=48:p=0,asettb=1/1000{framed}"));
    }
    runtime.verify()?;
    let identity = digest(&serde_json::to_vec(&(
        request,
        &runtime.reference,
        &access.wave,
        sources.iter().map(|s| &s.reference).collect::<Vec<_>>(),
        effect_start,
        duck_start,
        duck_end,
        &graph.filters,
    ))?);
    Ok(Prepared {
        inputs,
        request: request.clone(),
        runtime,
        sources,
        samples: edition.duration_samples,
        effect_start,
        duck_start,
        duck_end,
        graph,
        identity,
    })
}

fn sample(raw: &[u8], index: usize) -> i64 {
    let i = index * 3;
    i64::from(i32::from_le_bytes([
        raw[i],
        raw[i + 1],
        raw[i + 2],
        if raw[i + 2] & 128 == 0 { 0 } else { 255 },
    ]))
}
fn gained(raw: &[u8], index: usize, gain: f64, fade: u64) -> f64 {
    let frame = index as u64 / 2;
    let frames = raw.len() as u64 / 6;
    let envelope = if fade == 0 {
        1.0
    } else {
        (frame as f64 / fade as f64)
            .min(1.0)
            .min((frames - frame) as f64 / fade as f64)
    };
    sample(raw, index) as f64 * gain * envelope
}
/// Separate arithmetic oracle; never invokes the graph, FFmpeg or producer.
fn verify_pcm(
    prepared: &Prepared<'_>,
    root: &Path,
    tick: &mut impl FnMut() -> Result<()>,
) -> Result<BTreeMap<String, FileRef>> {
    let mut outputs = BTreeMap::new();
    let bed_gain = 10f64.powf(prepared.request.intent.bed_gain_db / 20.0);
    let duck_gain = 10f64
        .powf((prepared.request.intent.bed_gain_db + prepared.request.intent.duck_gain_db) / 20.0);
    let effect_gain = 10f64.powf(prepared.request.intent.effect_gain_db / 20.0);
    for (bus, name) in OUTPUT_NAMES.iter().enumerate() {
        tick()?;
        let bytes = producer::bounded(&root.join(name), 8 * 1024 * 1024)?;
        let pcm = crate::song_edition::wave::pcm24(&bytes)
            .map_err(|_| anyhow::anyhow!("spoken layered output format invalid"))?;
        require(
            pcm.len() as u64 == prepared.samples * 6,
            "spoken layered output duration differs",
        )?;
        for index in 0..pcm.len() / 3 {
            if index % 8192 == 0 {
                tick()?;
            }
            let frame = index as u64 / 2;
            let d = sample(&prepared.sources[0].pcm, index) as f64;
            let m = if index * 3 < prepared.sources[1].pcm.len() {
                let gain = if prepared.duck_start <= frame && frame < prepared.duck_end {
                    duck_gain
                } else {
                    bed_gain
                };
                gained(
                    &prepared.sources[1].pcm,
                    index,
                    gain,
                    prepared.request.intent.fade_samples,
                )
            } else {
                0.0
            };
            let e = if frame >= prepared.effect_start
                && frame < prepared.effect_start + prepared.sources[2].pcm.len() as u64 / 6
            {
                gained(
                    &prepared.sources[2].pcm,
                    index - prepared.effect_start as usize * 2,
                    effect_gain,
                    prepared.request.intent.fade_samples,
                )
            } else {
                0.0
            };
            let expected = [d, m, e, d + m + e][bus];
            let actual = sample(pcm, index);
            let tolerance = if bus == 0 || expected == 0.0 {
                0.0
            } else if bus == 3 {
                4.0
            } else {
                2.0
            };
            require(
                (actual as f64 - expected).abs() <= tolerance && actual.abs() <= 7_476_354,
                "spoken layered PCM differs or exceeds peak",
            )?;
        }
        outputs.insert(
            (*name).into(),
            FileRef {
                path: name.into(),
                sha256: digest(&bytes),
                bytes: bytes.len() as u64,
            },
        );
    }
    Ok(outputs)
}

// The sealed parent witness was independently checked during preparation.
// Reread the bytes actually consumed here, rather than rechecking unrelated
// parent editions for every use of the same immutable dialogue candidate.
fn revalidate(prepared: &Prepared<'_>) -> Result<()> {
    let dialogue_provider = prepared.inputs.dialogue_candidate.to_path_buf();
    for control in [
        &prepared.request.sonic_catalog,
        &prepared.request.sonic_request,
    ] {
        producer::read_source(prepared.inputs.layers, control, &mut || Ok(()))?;
    }
    for (index, original) in prepared.sources.iter().enumerate() {
        let inputs: &dyn InputFiles = if index == 0 {
            &dialogue_provider
        } else {
            prepared.inputs.layers
        };
        let source = source(inputs, original.reference.clone())?;
        require(
            source.pcm == original.pcm && source.path == original.path,
            "spoken layer consumed bytes/path changed",
        )?;
    }
    prepared.runtime.verify()
}

pub fn produce(
    prepared: &Prepared<'_>,
    output: &Path,
    timeout: Duration,
    mut cancellation: impl FnMut() -> Result<()>,
) -> Result<Receipt> {
    require(
        timeout <= Duration::from_secs(30),
        "spoken layer deadline exceeds profile",
    )?;
    let deadline = Instant::now() + timeout;
    let mut tick = || {
        cancellation()?;
        require(Instant::now() < deadline, "spoken layer deadline elapsed")
    };
    tick()?;
    revalidate(prepared)?;
    let fresh = prepared;
    tick()?;
    let mut filters = fresh.graph.filters.clone();
    let stems = fresh
        .graph
        .stems
        .as_ref()
        .context("spoken layer stems missing")?;
    for label in [
        &stems.pre_master,
        &stems.no_score,
        &stems.mono_review,
        &stems.small_speaker_review,
    ] {
        filters.push(format!("[{label}]anullsink"));
    }
    let filter_graph = filters.join(";");
    require(
        filter_graph.len() <= 8192
            && output.as_os_str().len() <= 512
            && fresh
                .sources
                .iter()
                .all(|s| s.path.as_os_str().len() <= 1024),
        "spoken layer command/path budget exceeded",
    )?;
    let mut owner = producer::OutputOwner::reserve(output)?;
    let stage = tempfile::tempdir_in(output)
        .map_err(|_| anyhow::anyhow!("spoken layer staging unavailable"))?;
    let mut command = Command::new(&fresh.runtime.path);
    command.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-n",
        "-filter_complex_threads",
        "1",
    ]);
    for source in &fresh.sources {
        command.arg("-i").arg(&source.path);
    }
    command.arg("-filter_complex").arg(&filter_graph);
    for (label, name) in [
        &stems.dialogue,
        &stems.music,
        &stems.effects,
        &fresh.graph.final_label,
    ]
    .into_iter()
    .zip(OUTPUT_NAMES)
    {
        command
            .args([
                "-map",
                &format!("[{label}]"),
                "-c:a",
                "pcm_s24le",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-bitexact",
                "-fs",
                "8388608",
            ])
            .arg(stage.path().join(name));
    }
    crate::music_album::process::run(&mut command, deadline, |mut stdout| {
        use std::io::Read;
        let mut limit = Vec::new();
        stdout.by_ref().take(65537).read_to_end(&mut limit)?;
        require(limit.len() <= 65536, "spoken layer stdout budget exceeded")
    })
    .map_err(|_| anyhow::anyhow!("spoken layer renderer failed or timed out"))?;
    let outputs = verify_pcm(fresh, stage.path(), &mut tick)?;
    revalidate(prepared)?;
    let receipt = Receipt {
        schema: RECEIPT_SCHEMA.into(),
        mix_identity_sha256: fresh.identity.clone(),
        request: fresh.request.clone(),
        runtime: fresh.runtime.reference.clone(),
        dialogue: fresh.sources[0].reference.clone(),
        samples: fresh.samples,
        effect_start_sample: fresh.effect_start,
        duck_start_sample: fresh.duck_start,
        duck_end_sample: fresh.duck_end,
        outputs,
        authority_verified: false,
        release_authorized: false,
    };
    for name in OUTPUT_NAMES {
        producer::persist_file(
            &mut owner,
            name,
            &producer::bounded(&stage.path().join(name), 8 * 1024 * 1024)?,
            &mut tick,
        )?;
    }
    producer::persist_file(
        &mut owner,
        "receipt.json",
        &serde_json::to_vec_pretty(&receipt)?,
        &mut tick,
    )?;
    owner.finish();
    Ok(receipt)
}

pub fn check(prepared: &Prepared<'_>, candidate: &Path, timeout: Duration) -> Result<CheckSummary> {
    require(
        timeout <= Duration::from_secs(30),
        "spoken layer check deadline exceeds profile",
    )?;
    let deadline = Instant::now() + timeout;
    let mut tick = || {
        require(
            Instant::now() < deadline,
            "spoken layer check deadline elapsed",
        )
    };
    tick()?;
    revalidate(prepared)?;
    let fresh = prepared;
    let raw = producer::bounded(&candidate.join("receipt.json"), 1024 * 1024)?;
    let receipt: Receipt = serde_json::from_slice(&raw)
        .map_err(|_| anyhow::anyhow!("spoken layer receipt invalid"))?;
    require(
        receipt.schema == RECEIPT_SCHEMA
            && receipt.mix_identity_sha256 == fresh.identity
            && serde_json::to_value(&receipt.request)? == serde_json::to_value(&fresh.request)?
            && serde_json::to_value(&receipt.runtime)?
                == serde_json::to_value(&fresh.runtime.reference)?
            && serde_json::to_value(&receipt.dialogue)?
                == serde_json::to_value(&fresh.sources[0].reference)?
            && receipt.samples == fresh.samples
            && receipt.effect_start_sample == fresh.effect_start
            && receipt.duck_start_sample == fresh.duck_start
            && receipt.duck_end_sample == fresh.duck_end
            && !receipt.authority_verified
            && !receipt.release_authorized,
        "spoken layer receipt original binding differs",
    )?;
    let outputs = verify_pcm(fresh, candidate, &mut tick)?;
    require(
        serde_json::to_value(outputs)? == serde_json::to_value(receipt.outputs)?,
        "spoken layer output pins differ",
    )?;
    tick()?;
    Ok(CheckSummary {
        independent_pcm_check_passed: true,
        authority_verified: false,
        release_authorized: false,
    })
}
