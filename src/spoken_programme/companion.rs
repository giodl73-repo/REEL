//! Checked spoken clocks for existing admitted browser compositions.
//! No asset definitions, store, voice service, player or publication authority.
use super::{checker, compiler, producer};
use crate::{
    browser_composition::{self, BrowserCompositionManifest, HashedAsset, PcmWindow},
    cue_relative::MarkerKind,
    scene_delivery::{FileRef, InputFiles},
    tig_browser,
};
use anyhow::{Context, Result, ensure};
use reel_assembly::scene_authoring::spoken_phrase_sample;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::{Duration, Instant},
};

pub const SCHEMA: &str = "reel.spoken-companion.v1";
pub const INDEX_SCHEMA: &str = "reel.spoken-companion-index.v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub edition_ids: Vec<String>,
    pub svg: FileRef,
    pub width: u32,
    pub height: u32,
    pub events: Vec<Event>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub bindings: Vec<Binding>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub edition_id: String,
    pub anchor: Anchor,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Anchor {
    Phrase {
        performance_id: String,
        marker_id: String,
        phrase: String,
        occurrence: Option<usize>,
    },
    EngineeringMarker {
        performance_id: String,
        marker_id: String,
    },
}
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub spoken: checker::CheckRequest<'a>,
    pub sources: &'a dyn InputFiles,
    pub candidate: &'a dyn InputFiles,
    pub visuals: &'a dyn InputFiles,
    pub access: &'a dyn InputFiles,
    pub access_manifest: &'a [u8],
}
pub struct Prepared<'a> {
    request: Request,
    inputs: Inputs<'a>,
    editions: Vec<Composition>,
    index: Vec<u8>,
}
/// Private controls for admission through the caller's existing asset system.
/// This is not a Tig checkpoint, custody receipt or freely supplied evidence.
pub struct Composition {
    edition_id: String,
    manifest: BrowserCompositionManifest,
    svg: FileRef,
    wave: FileRef,
    access_sha256: String,
    clock: Vec<u8>,
    evidence: Vec<u8>,
}
impl Composition {
    pub fn edition_id(&self) -> &str {
        &self.edition_id
    }
    pub fn manifest(&self) -> &BrowserCompositionManifest {
        &self.manifest
    }
    pub(crate) fn access_sha256(&self) -> &str {
        &self.access_sha256
    }
    pub fn clock_bytes(&self) -> &[u8] {
        &self.clock
    }
    pub fn evidence_bytes(&self) -> &[u8] {
        &self.evidence
    }
}
impl Prepared<'_> {
    pub fn compositions(&self) -> &[Composition] {
        &self.editions
    }
}
#[derive(Serialize)]
pub struct Summary {
    pub editions: usize,
    pub events: usize,
    pub source_bindings_checked: bool,
    pub acoustic_word_fidelity_verified: bool,
    pub authority_verified: bool,
    pub device_playback_checked: bool,
    pub release_authorized: bool,
}
fn sha(raw: &[u8]) -> String {
    crate::production::sha256_bytes(raw)
}
fn pin(path: &str, raw: &[u8]) -> FileRef {
    FileRef {
        path: path.into(),
        sha256: sha(raw),
        bytes: raw.len() as u64,
    }
}
fn name(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn bounded_anchor(anchor: &Anchor) -> bool {
    let (performance, marker, phrase_ok) = match anchor {
        Anchor::Phrase {
            performance_id,
            marker_id,
            phrase,
            occurrence,
        } => (
            performance_id,
            marker_id,
            !phrase.trim().is_empty() && phrase.len() <= 8192 && *occurrence != Some(0),
        ),
        Anchor::EngineeringMarker {
            performance_id,
            marker_id,
        } => (performance_id, marker_id, true),
    };
    name(performance) && name(marker) && phrase_ok
}
fn read(files: &dyn InputFiles, file: &FileRef, max: u64) -> Result<Vec<u8>> {
    ensure!(
        file.bytes > 0 && file.bytes <= max,
        "companion input exceeds byte profile"
    );
    files.validate_reference_name(file)?;
    let bytes = files.control_bytes(file)?;
    ensure!(
        bytes.len() as u64 == file.bytes && sha(&bytes) == file.sha256,
        "companion original input bytes differ"
    );
    Ok(bytes)
}
fn end(timeout: Duration) -> Result<Instant> {
    ensure!(
        timeout <= Duration::from_secs(30),
        "companion metadata deadline exceeds profile"
    );
    Ok(Instant::now() + timeout)
}
fn tick(deadline: Instant, cancellation: &mut impl FnMut() -> Result<()>) -> Result<()> {
    cancellation()?;
    ensure!(Instant::now() < deadline, "companion deadline elapsed");
    Ok(())
}
fn phrase_sample(
    evidence: &compiler::AlignmentEvidence,
    text: &str,
    marker_id: &str,
    phrase: &str,
    occurrence: Option<usize>,
) -> Result<u64> {
    let compiler::Observation::WordEvidence { timing, .. } = &evidence.observation else {
        anyhow::bail!("companion phrase requires original word evidence");
    };
    let marker = evidence
        .markers
        .iter()
        .find(|m| m.id == marker_id)
        .context("companion phrase marker missing")?;
    ensure!(
        matches!(marker.kind, MarkerKind::Phrase),
        "companion phrase marker kind differs"
    );
    let sample = spoken_phrase_sample(timing, text, phrase, occurrence)?;
    ensure!(
        sample == marker.start_sample,
        "companion phrase and marker entrance differ"
    );
    Ok(sample)
}
fn resolve(
    anchor: &Anchor,
    edition: &compiler::CompiledEdition,
    sources: &dyn InputFiles,
) -> Result<Value> {
    let (performance_id, marker_id) = match anchor {
        Anchor::Phrase {
            performance_id,
            marker_id,
            ..
        }
        | Anchor::EngineeringMarker {
            performance_id,
            marker_id,
        } => (performance_id, marker_id),
    };
    let cues: Vec<_> = edition
        .cues
        .iter()
        .filter(|c| &c.performance_id == performance_id)
        .collect();
    ensure!(
        cues.len() == 1,
        "companion performance missing or ambiguous"
    );
    let cue = cues[0];
    let consumed = edition
        .recipe
        .cues
        .iter()
        .find(|c| &c.performance.id == performance_id)
        .context("companion original performance missing")?;
    let p = &consumed.performance;
    let markers: Vec<_> = edition
        .markers
        .iter()
        .filter(|m| &m.performance_id == performance_id && &m.marker.id == marker_id)
        .collect();
    ensure!(
        markers.len() == 1,
        "companion placed marker missing or ambiguous"
    );
    let placed = &markers[0].marker;
    let (basis, phrase) = match anchor {
        Anchor::Phrase {
            phrase, occurrence, ..
        } => {
            ensure!(
                !cue.engineering_signal && phrase.len() <= 8192,
                "companion phrase cannot borrow engineering evidence"
            );
            let raw = read(sources, &p.alignment, super::MAX_DECLARATION_BYTES as u64)?;
            let evidence: compiler::AlignmentEvidence = serde_json::from_slice(&raw)?;
            let sample = phrase_sample(
                &evidence,
                &consumed.script.text,
                marker_id,
                phrase,
                *occurrence,
            )?;
            let expected = sample
                .checked_sub(cue.source_start_sample)
                .and_then(|n| cue.start_sample.checked_add(n))
                .context("companion phrase outside selected take")?;
            ensure!(
                expected == placed.start_sample,
                "companion phrase placement differs"
            );
            (
                "word-evidence-not-listening-approval",
                Some(phrase.as_str()),
            )
        }
        Anchor::EngineeringMarker { .. } => {
            ensure!(
                cue.engineering_signal,
                "companion engineering marker requires engineering signal"
            );
            ("engineering-marker-not-spoken-words", None)
        }
    };
    Ok(
        json!({"anchor":anchor,"basis":basis,"phrase":phrase,"sample":placed.start_sample,
        "alignment":p.alignment,"performance_source":p.source}),
    )
}
fn derive(
    request: &Request,
    inputs: Inputs<'_>,
    deadline: Instant,
    cancellation: &mut impl FnMut() -> Result<()>,
) -> Result<(Vec<Composition>, Vec<u8>)> {
    tick(deadline, cancellation)?;
    ensure!(
        request.schema == SCHEMA
            && (1..=2).contains(&request.edition_ids.len())
            && request.events.len() == 2
            && request.width > 0
            && request.width <= 640
            && request.height > 0
            && request.height <= 360
            && request.svg.path.as_os_str().len() <= 4096
            && request.svg.sha256.len() == 64,
        "companion request exceeds profile"
    );
    // Bound nested typed input before serializing it or asking any provider.
    ensure!(
        request
            .events
            .iter()
            .all(|event| event.bindings.len() == request.edition_ids.len()
                && event
                    .bindings
                    .iter()
                    .all(|b| name(&b.edition_id) && bounded_anchor(&b.anchor))),
        "companion binding closure exceeds profile"
    );
    let mut editions_seen = BTreeSet::new();
    ensure!(
        request
            .edition_ids
            .iter()
            .all(|id| name(id) && editions_seen.insert(id)),
        "companion edition missing or duplicated"
    );
    let mut events_seen = BTreeSet::new();
    ensure!(
        request.events.iter().all(|e| name(&e.id)
            && !["start", "end"].contains(&e.id.as_str())
            && events_seen.insert(&e.id)),
        "companion event missing, reserved or duplicated"
    );
    for event in &request.events {
        let ids: BTreeSet<_> = event.bindings.iter().map(|b| &b.edition_id).collect();
        ensure!(
            ids == editions_seen && ids.len() == event.bindings.len(),
            "companion language binding closure differs"
        );
    }
    ensure!(
        serde_json::to_vec(request)?.len() <= 65536,
        "companion request exceeds byte profile"
    );
    read(inputs.visuals, &request.svg, 65536)?;
    let mut checked = checker::check(
        inputs.spoken,
        inputs.sources,
        inputs.candidate,
        producer::ProductionControl {
            timeout: deadline.saturating_duration_since(Instant::now()),
            cancellation: &mut *cancellation,
        },
    )?;
    checker::verify_access(&mut checked, inputs.access_manifest, inputs.access, || {
        tick(deadline, cancellation)
    })?;
    let mut compositions = Vec::new();
    let mut entries = Vec::new();
    for id in &request.edition_ids {
        tick(deadline, cancellation)?;
        let edition = checked
            .compiled_plan()
            .editions
            .iter()
            .find(|e| &e.recipe.edition_id == id)
            .context("companion selected edition absent")?;
        let access = checked
            .access()
            .editions
            .iter()
            .find(|e| &e.edition_id == id)
            .context("companion checked text access absent")?;
        ensure!(
            edition.duration_samples > 0
                && edition.duration_samples <= 480000
                && edition.duration_samples % 2000 == 0,
            "companion duration must fit whole frames within ten seconds"
        );
        let mut resolved = Vec::new();
        let mut anchors = vec![json!({"id":"start","sample":0})];
        for event in &request.events {
            let binding = event.bindings.iter().find(|b| &b.edition_id == id).unwrap();
            let mut value = resolve(&binding.anchor, edition, inputs.sources)?;
            anchors.push(json!({"id":event.id,"sample":value["sample"]}));
            value["event_id"] = json!(event.id);
            resolved.push(value);
        }
        anchors.push(json!({"id":"end","sample":edition.duration_samples}));
        let evidence = serde_json::to_vec_pretty(
            &json!({"schema":"reel.spoken-companion-evidence.v1",
            "recipe":edition.recipe,"compiled_recipe_sha256":edition.recipe_sha256,"access":access,"events":resolved,
            "svg":request.svg,"acoustic_word_fidelity_verified":false,"authority_verified":false}),
        )?;
        ensure!(
            evidence.len() <= 1024 * 1024,
            "companion evidence exceeds profile"
        );
        let clock = serde_json::to_vec_pretty(
            &json!({"schema":"reel.presentation-source-clock.v1",
            "source_sha256":access.wave.sha256,"sample_rate":48000,"total_samples":edition.duration_samples,
            "evidence":pin("evidence.json", &evidence),"anchors":anchors}),
        )?;
        ensure!(clock.len() <= 65536, "companion clock exceeds profile");
        let manifest = BrowserCompositionManifest {
            schema: browser_composition::MANIFEST_SCHEMA.into(),
            source: HashedAsset {
                path: "opening.svg".into(),
                sha256: request.svg.sha256.clone(),
            },
            assets: vec![
                HashedAsset {
                    path: "clock.json".into(),
                    sha256: sha(&clock),
                },
                HashedAsset {
                    path: "evidence.json".into(),
                    sha256: sha(&evidence),
                },
            ],
            width: request.width,
            height: request.height,
            fps: 24,
            frame_count: edition.duration_samples / 2000,
            dynamic_regions: vec![],
            audio: Some(HashedAsset {
                path: "audio.wav".into(),
                sha256: access.wave.sha256.clone(),
            }),
            source_clock: Some(HashedAsset {
                path: "clock.json".into(),
                sha256: sha(&clock),
            }),
            audio_window: Some(PcmWindow {
                source_samples: edition.duration_samples,
                start_sample: 0,
                end_sample: edition.duration_samples,
                fade_in_samples: 0,
                fade_out_samples: 0,
            }),
            lossless_master: None,
        };
        let event = browser_composition::source_clock_event(&manifest, |asset, _| {
            match asset.path.as_str() {
                "clock.json" => Ok(clock.clone()),
                "evidence.json" => Ok(evidence.clone()),
                _ => anyhow::bail!("companion undeclared control"),
            }
        })?
        .context("companion source clock absent")?;
        let frames = event["anchorFrames"]
            .as_array()
            .context("companion clock frames absent")?;
        ensure!(
            frames
                .windows(2)
                .all(|w| w[0].as_u64().unwrap() < w[1].as_u64().unwrap()),
            "companion transitions must have distinct interior frames"
        );
        let n = compositions.len();
        entries.push(json!({"edition_id":id,"language":edition.recipe.language,"manifest":manifest,
            "svg":request.svg,"wave":access.wave,"text_access":access.transcript,
            "clock":pin(&format!("edition-{n:03}.clock.json"), &clock),
            "evidence":pin(&format!("edition-{n:03}.evidence.json"), &evidence),"frame_event":event}));
        compositions.push(Composition {
            edition_id: id.clone(),
            manifest,
            svg: request.svg.clone(),
            wave: access.wave.clone(),
            access_sha256: access.transcript.sha256.clone(),
            clock,
            evidence,
        });
    }
    let index = serde_json::to_vec_pretty(
        &json!({"schema":INDEX_SCHEMA,"request":request,"editions":entries,
        "acoustic_word_fidelity_verified":false,"authority_verified":false,"device_playback_checked":false,"release_authorized":false}),
    )?;
    ensure!(
        index.len() <= 1024 * 1024,
        "companion index exceeds profile"
    );
    tick(deadline, cancellation)?;
    Ok((compositions, index))
}
pub fn prepare<'a>(
    request: Request,
    inputs: Inputs<'a>,
    timeout: Duration,
) -> Result<Prepared<'a>> {
    let (editions, index) = derive(&request, inputs, end(timeout)?, &mut || Ok(()))?;
    Ok(Prepared {
        request,
        inputs,
        editions,
        index,
    })
}
fn recheck(
    prepared: &Prepared<'_>,
    deadline: Instant,
    cancellation: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    let (_, index) = derive(&prepared.request, prepared.inputs, deadline, cancellation)?;
    ensure!(
        index == prepared.index,
        "companion original controls changed"
    );
    Ok(())
}
fn summary(p: &Prepared<'_>) -> Summary {
    Summary {
        editions: p.editions.len(),
        events: p.request.events.len(),
        source_bindings_checked: true,
        acoustic_word_fidelity_verified: false,
        authority_verified: false,
        device_playback_checked: false,
        release_authorized: false,
    }
}
pub fn produce(
    prepared: &Prepared<'_>,
    output: &Path,
    mut control: producer::ProductionControl<impl FnMut() -> Result<()>>,
) -> Result<Summary> {
    let deadline = end(control.timeout)?;
    recheck(prepared, deadline, &mut control.cancellation)?;
    let mut owner = producer::OutputOwner::reserve(output)?;
    let mut check = || tick(deadline, &mut control.cancellation);
    for (n, c) in prepared.editions.iter().enumerate() {
        producer::persist_file(
            &mut owner,
            &format!("edition-{n:03}.clock.json"),
            &c.clock,
            &mut check,
        )?;
        producer::persist_file(
            &mut owner,
            &format!("edition-{n:03}.evidence.json"),
            &c.evidence,
            &mut check,
        )?;
    }
    producer::persist_file(&mut owner, "companion.json", &prepared.index, &mut check)?;
    owner.finish();
    Ok(summary(prepared))
}
pub fn check(prepared: &Prepared<'_>, output: &Path, timeout: Duration) -> Result<Summary> {
    let deadline = end(timeout)?;
    recheck(prepared, deadline, &mut || Ok(()))?;
    let mut expected = BTreeMap::from([("companion.json".to_owned(), prepared.index.as_slice())]);
    for (n, c) in prepared.editions.iter().enumerate() {
        expected.insert(format!("edition-{n:03}.clock.json"), &c.clock);
        expected.insert(format!("edition-{n:03}.evidence.json"), &c.evidence);
    }
    let names: BTreeSet<_> = fs::read_dir(output)?
        .take(6)
        .map(|e| e.map(|e| e.file_name()))
        .collect::<std::io::Result<_>>()?;
    ensure!(
        names == expected.keys().map(std::ffi::OsString::from).collect(),
        "companion output namespace differs"
    );
    for (name, raw) in expected {
        ensure!(
            read(&output.to_path_buf(), &pin(&name, raw), 1024 * 1024)? == raw,
            "companion output bytes differ"
        );
    }
    tick(deadline, &mut || Ok(()))?;
    Ok(summary(prepared))
}
fn composition<'a>(prepared: &'a Prepared<'_>, id: &str) -> Result<&'a Composition> {
    prepared
        .editions
        .iter()
        .find(|c| c.edition_id == id)
        .context("companion browser edition absent")
}
fn validate_composition(
    c: &Composition,
    plan: &tig_browser::Prepared,
    files: &dyn InputFiles,
    deadline: Instant,
    cancellation: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    ensure!(
        serde_json::to_vec(&plan.manifest)? == serde_json::to_vec(&c.manifest)?
            && plan.manifest_bytes == serde_json::to_vec(&c.manifest)?,
        "companion browser manifest differs from original clock"
    );
    ensure!(
        plan.inputs.len() == 4,
        "companion browser original input closure differs"
    );
    // Candidate filenames and admitted IDs are separate provider namespaces.
    // Bind exact objects without rewriting original provenance or receipts.
    for (alias, original) in [("opening.svg", &c.svg), ("audio.wav", &c.wave)] {
        let admitted = plan
            .inputs
            .get(alias)
            .context("companion browser original input absent")?;
        ensure!(
            admitted.sha256 == original.sha256 && admitted.bytes == original.bytes,
            "companion browser original object differs"
        );
        files.validate_reference_name(admitted)?;
    }
    tick(deadline, cancellation)?;
    read(files, &plan.inputs["opening.svg"], 65536)?;
    producer::read_source(files, &plan.inputs["audio.wav"], &mut || {
        tick(deadline, cancellation)
    })?;
    for (name, raw) in [("clock.json", &c.clock), ("evidence.json", &c.evidence)] {
        let file = plan
            .inputs
            .get(name)
            .context("companion browser control absent")?;
        ensure!(
            read(files, file, 1024 * 1024)? == *raw,
            "companion browser control differs from original evidence"
        );
    }
    tig_browser::capture::validate_resources(plan)?;
    tick(deadline, cancellation)?;
    Ok(())
}
pub fn validate_browser(
    prepared: &Prepared<'_>,
    edition_id: &str,
    plan: &tig_browser::Prepared,
    files: &dyn InputFiles,
    timeout: Duration,
) -> Result<Summary> {
    let deadline = end(timeout)?;
    recheck(prepared, deadline, &mut || Ok(()))?;
    validate_composition(
        composition(prepared, edition_id)?,
        plan,
        files,
        deadline,
        &mut || Ok(()),
    )?;
    tick(deadline, &mut || Ok(()))?;
    Ok(summary(prepared))
}

/// Existing host enrollment, held byte/program guards and peak reservation are
/// mandatory. One selected edition per bounded operation; no cache/adoption.
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub fn render(
    prepared: &Prepared<'_>,
    edition_id: &str,
    plan: &tig_browser::Prepared,
    files: &dyn InputFiles,
    programs: &crate::media_runtime::BoundPrograms,
    browser: &crate::media_runtime::BoundTool,
    workspace: &Path,
    output: &Path,
    mut cancellation: impl FnMut() -> Result<()>,
) -> Result<tig_browser::render::Rendered> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let metadata_end = Instant::now() + Duration::from_secs(30);
    recheck(prepared, metadata_end, &mut cancellation)?;
    tick(deadline, &mut cancellation)?;
    validate_composition(
        composition(prepared, edition_id)?,
        plan,
        files,
        metadata_end,
        &mut cancellation,
    )?;
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(60));
    let limits = tig_browser::render::Limits {
        capture: tig_browser::capture::Limits {
            max_requests: 32,
            max_resource_bytes: 1024 * 1024,
            max_served_bytes: 4 * 1024 * 1024,
            max_message_bytes: 4 * 1024 * 1024,
            max_messages: 4096,
            max_png_bytes: 1024 * 1024,
            max_frames: 240,
            max_canvas_pixels: 640 * 360,
            max_elapsed_ms: remaining.as_millis() as u64,
            max_io_wait_ms: 30000,
        },
        max_workspace_bytes: 128 * 1024 * 1024,
        max_output_bytes: 16 * 1024 * 1024,
        browser_memory_bytes: 2 * 1024 * 1024 * 1024,
        browser_processes: 64,
        encoder_memory_bytes: 256 * 1024 * 1024,
        encoder_processes: 4,
    };
    tig_browser::render::native(
        plan,
        files,
        programs,
        browser,
        workspace,
        output,
        &limits,
        false,
        || tick(deadline, &mut cancellation),
    )
}

#[cfg(test)]
mod tests;
