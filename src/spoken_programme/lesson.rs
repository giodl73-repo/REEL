//! Source-bound exercise navigation; no playback or musical/voice approval.
use super::{producer, reading};
use crate::{
    music_score,
    scene_delivery::{FileRef, InputFiles},
};
use anyhow::{Context, Result, ensure};
use reel_music::{
    model::{self, MusicModel},
    time::RoundingMode,
    validation_inputs::ValidationInputs,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const SCHEMA: &str = "reel.lesson.v1";
pub const INDEX_SCHEMA: &str = "reel.lesson-index.v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub id: String,
    pub reading_index: FileRef,
    pub edition_ids: Vec<String>,
    pub instructions: Vec<Instruction>,
    pub score_files: Vec<FileRef>,
    pub model: PathBuf,
    pub plan: PathBuf,
    pub receipt: PathBuf,
    pub packet: PathBuf,
    pub score_contract_sha256: String,
    pub ppq: u32,
    pub frame_rounding: RoundingMode,
    pub occurrences: Vec<Occurrence>,
    pub anchors: Vec<Anchor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instruction {
    pub language: String,
    pub source: FileRef,
    pub authority: FileRef,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    pub id: String,
    pub section_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub id: String,
    pub occurrence_id: String,
    pub note_id: String,
    pub source_tick: u64,
}
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub reading: &'a reading::Prepared<'a>,
    pub reading_directory: &'a Path,
    pub assets: &'a dyn InputFiles,
}
pub struct Prepared<'a> {
    request: Request,
    inputs: Inputs<'a>,
    index: Vec<u8>,
}
#[derive(Serialize)]
pub struct Summary {
    pub editions: usize,
    pub occurrences: usize,
    pub anchors: usize,
    pub source_bindings_checked: bool,
    pub musical_accuracy_verified: bool,
    pub authority_verified: bool,
    pub device_playback_checked: bool,
    pub release_authorized: bool,
}
fn sha(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256
}
fn read(provider: &dyn InputFiles, pin: &FileRef, max: u64) -> Result<Vec<u8>> {
    ensure!(
        pin.bytes > 0
            && pin.bytes <= max
            && pin.sha256.len() == 64
            && pin
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "lesson input pin exceeds profile"
    );
    provider.validate_reference_name(pin)?;
    let raw = provider.control_bytes(pin)?;
    ensure!(
        raw.len() as u64 == pin.bytes && sha(&raw) == pin.sha256,
        "lesson original input bytes differ"
    );
    Ok(raw)
}
fn reading_bytes(request: &Request, inputs: Inputs<'_>) -> Result<Vec<u8>> {
    read(
        &inputs.reading_directory.to_path_buf(),
        &request.reading_index,
        4 * 1024 * 1024,
    )
}
fn profile(model: &MusicModel, request: &Request) -> Result<()> {
    ensure!(
        model.parts.len() == 1
            && !model.parts[0].notes.is_empty()
            && model.parts[0].notes.len() <= 32
            && !matches!(model.parts[0].role, model::PartRole::Vocal)
            && model.parts[0]
                .notes
                .iter()
                .all(|n| n.voice == model.parts[0].notes[0].voice)
            && model.tempo_map.len() == 1
            && model.meter_map.len() == 1
            && model.meter_map[0].numerator == 4
            && model.meter_map[0].denominator == 4
            && model.expressive_timing.is_empty()
            && model.lyric_layers.is_empty(),
        "lesson score profile unsupported"
    );
    let score = model
        .instrumental_score
        .as_ref()
        .context("lesson requires an editable instrumental score")?;
    ensure!(
        score.parts.len() == 1
            && score.pickup_ticks > 0
            && model
                .form
                .first()
                .is_some_and(|f| f.range.start == 0 && f.range.end == score.pickup_ticks)
            && model.musical_timebase.pulses_per_quarter == request.ppq
            && matches!(request.frame_rounding, RoundingMode::HalfAwayFromZero),
        "lesson pickup, timebase or rounding differs"
    );
    ensure!(
        music_score::tick_to_sample(model.duration_ticks, model, 48000)? <= 480000,
        "lesson reference exceeds ten seconds"
    );
    Ok(())
}
fn derive(request: &Request, inputs: Inputs<'_>, deadline: Instant) -> Result<Vec<u8>> {
    let tick = || -> Result<()> {
        ensure!(Instant::now() < deadline, "lesson deadline elapsed");
        Ok(())
    };
    tick()?;
    ensure!(
        request.schema == SCHEMA
            && name(&request.id)
            && (1..=4).contains(&request.edition_ids.len())
            && (1..=4).contains(&request.instructions.len())
            && (1..=32).contains(&request.score_files.len())
            && (1..=16).contains(&request.occurrences.len())
            && request.anchors.len() <= 64,
        "lesson request exceeds profile"
    );
    let mut files = BTreeMap::new();
    let mut bytes = 0u64;
    for pin in &request.score_files {
        ensure!(
            pin.bytes <= 1024 * 1024 && files.insert(pin.path.clone(), pin).is_none(),
            "lesson score namespace conflicts or exceeds profile"
        );
        bytes = bytes
            .checked_add(pin.bytes)
            .context("lesson score bytes overflow")?;
    }
    ensure!(
        bytes <= 16 * 1024 * 1024,
        "lesson score total exceeds profile"
    );
    ensure!(
        request.receipt == request.packet.join("receipt.json"),
        "lesson packet receipt name differs"
    );
    ensure!(
        files
            .get(&request.model)
            .is_some_and(|p| p.bytes <= 256 * 1024),
        "lesson model absent or oversized"
    );
    let mut capture = |path: &Path| -> Result<Vec<u8>> {
        tick()?;
        read(
            inputs.assets,
            files.get(path).context("lesson undeclared score input")?,
            1024 * 1024,
        )
    };
    let mut score_inputs = ValidationInputs::admitted(&mut capture);
    let checked = model::validate_with_inputs(&request.model, &mut score_inputs)?;
    let model = model::load_with_inputs(&request.model, &mut score_inputs)?;
    ensure!(
        checked.contract_sha256 == request.score_contract_sha256,
        "lesson score contract is stale"
    );
    profile(&model, request)?;
    let mut seen = BTreeSet::new();
    let mut sections = BTreeSet::new();
    let mut prior_section = 0usize;
    let mut total = 0u64;
    let mut occurrences = Vec::new();
    let mut clocks = BTreeMap::new();
    for (i, occurrence) in request.occurrences.iter().enumerate() {
        ensure!(
            name(&occurrence.id) && seen.insert(&occurrence.id),
            "lesson occurrence missing or duplicated"
        );
        let section_index = model
            .form
            .iter()
            .position(|s| s.id == occurrence.section_id)
            .context("lesson unknown score section")?;
        ensure!(
            (i == 0 && section_index == 0)
                || (i > 0
                    && (section_index == prior_section || section_index == prior_section + 1)),
            "lesson occurrence order skips or rearranges source"
        );
        ensure!(
            section_index != 0 || i == 0,
            "lesson pickup may occur only once"
        );
        prior_section = section_index;
        sections.insert(section_index);
        let section = &model.form[section_index];
        let start = music_score::tick_to_sample(section.range.start, &model, 48000)?;
        let end = music_score::tick_to_sample(section.range.end, &model, 48000)?;
        ensure!(start < end, "lesson source section has no sample interval");
        let exercise_end = total
            .checked_add(end - start)
            .context("lesson exercise clock overflow")?;
        ensure!(
            exercise_end <= 1440000,
            "lesson exercise exceeds thirty seconds"
        );
        clocks.insert(occurrence.id.as_str(), (section, start, total));
        occurrences.push(serde_json::json!({"id":occurrence.id,"section_id":section.id,
            "source_ticks":[section.range.start,section.range.end],"source_samples":[start,end],"exercise_samples":[total,exercise_end]}));
        total = exercise_end;
    }
    ensure!(
        sections.len() == model.form.len(),
        "lesson traversal omits written sections"
    );
    let mut seen = BTreeSet::new();
    let mut anchors = Vec::new();
    for anchor in &request.anchors {
        ensure!(
            name(&anchor.id) && seen.insert(&anchor.id),
            "lesson anchor missing or duplicated"
        );
        let (section, source_start, exercise_start) = clocks
            .get(anchor.occurrence_id.as_str())
            .context("lesson anchor occurrence is absent or ambiguous")?;
        let note = model.parts[0]
            .notes
            .iter()
            .find(|n| n.id == anchor.note_id)
            .context("lesson anchor note is unknown")?;
        let note_end = note
            .start_tick
            .checked_add(note.duration_ticks)
            .context("lesson note tick overflow")?;
        ensure!(
            anchor.source_tick >= section.range.start
                && anchor.source_tick <= section.range.end
                && anchor.source_tick >= note.start_tick
                && anchor.source_tick <= note_end,
            "lesson anchor outside section or note"
        );
        let source = music_score::tick_to_sample(anchor.source_tick, &model, 48000)?;
        let exercise = exercise_start
            .checked_add(source - source_start)
            .context("lesson anchor sample overflow")?;
        anchors.push(serde_json::json!({"id":anchor.id,"occurrence_id":anchor.occurrence_id,"note_id":anchor.note_id,
            "source_tick":anchor.source_tick,"source_sample":source,"exercise_sample":exercise}));
    }
    music_score::check_with_inputs(
        &request.receipt,
        &request.plan,
        &request.model,
        &request.packet,
        &mut score_inputs,
    )?;
    ensure!(
        score_inputs.names().eq(files.keys().map(PathBuf::as_path)),
        "lesson score namespace includes unused inputs"
    );
    tick()?;
    let reading_raw = reading_bytes(request, inputs)?;
    let reading_index: serde_json::Value = serde_json::from_slice(&reading_raw)?;
    ensure!(
        reading_index["schema"] == reading::INDEX_SCHEMA,
        "lesson reading schema differs"
    );
    let entries = reading_index["editions"]
        .as_array()
        .context("lesson reading editions missing")?;
    let mut selected = BTreeSet::new();
    let mut languages = BTreeSet::new();
    let mut explanations = Vec::new();
    for id in &request.edition_ids {
        ensure!(selected.insert(id), "lesson explanation edition duplicated");
        let edition = entries
            .iter()
            .find(|e| e["edition_id"].as_str() == Some(id))
            .context("lesson explanation edition missing")?;
        let language = edition["language"]
            .as_str()
            .context("lesson explanation language missing")?;
        languages.insert(language.to_owned());
        explanations.push(serde_json::json!({"edition_id":id,"language":language,"compiled_recipe_sha256":edition["compiled_recipe_sha256"],
            "execution_recipe_sha256":edition["execution_recipe_sha256"],"duration_samples":edition["duration_samples"]}));
    }
    let mut instruction_languages = BTreeSet::new();
    let mut instructions = Vec::new();
    for instruction in &request.instructions {
        ensure!(
            instruction_languages.insert(instruction.language.clone()),
            "lesson instruction language duplicated"
        );
        let raw = read(inputs.assets, &instruction.source, 65536)?;
        let text = std::str::from_utf8(&raw).context("lesson instructions must be UTF-8")?;
        ensure!(!text.trim().is_empty(), "lesson instructions are empty");
        read(inputs.assets, &instruction.authority, 65536)?;
        instructions.push(serde_json::json!({"language":instruction.language,"source":instruction.source,"authority":instruction.authority,
            "text":text,"text_basis":"authored-instructions-not-spoken-evidence"}));
    }
    ensure!(
        languages == instruction_languages,
        "lesson instruction and explanation languages differ"
    );
    reading::check(
        inputs.reading,
        inputs.reading_directory,
        deadline.saturating_duration_since(Instant::now()),
    )?;
    let reference = files
        .get(&request.packet.join("rehearsal-guide.wav"))
        .context("lesson reference pin missing")?;
    let result = serde_json::to_vec_pretty(
        &serde_json::json!({"schema":INDEX_SCHEMA,"request":request,"explanations":explanations,
        "instructions":instructions,"score_contract_sha256":checked.contract_sha256,"reference":reference,
        "reference_clock":{"sample_rate":48000,"channels":1,"pcm_bits":16,"purpose":"timing-and-pitch-guide"},
        "occurrences":occurrences,"anchors":anchors,"exercise_samples":total,"exercise_clock":"virtual-index-only",
        "musical_accuracy_verified":false,"authority_verified":false,"device_playback_checked":false,"release_authorized":false}),
    )?;
    ensure!(result.len() <= 1024 * 1024, "lesson index exceeds profile");
    tick()?;
    Ok(result)
}
fn deadline(timeout: Duration) -> Result<Instant> {
    ensure!(
        timeout <= Duration::from_secs(30),
        "lesson deadline exceeds profile"
    );
    Ok(Instant::now() + timeout)
}
pub fn prepare<'a>(
    request: Request,
    inputs: Inputs<'a>,
    timeout: Duration,
) -> Result<Prepared<'a>> {
    ensure!(
        serde_json::to_vec(&request)?.len() <= 256 * 1024,
        "lesson request byte budget exceeded"
    );
    let index = derive(&request, inputs, deadline(timeout)?)?;
    Ok(Prepared {
        request,
        inputs,
        index,
    })
}
fn recheck(
    prepared: &Prepared<'_>,
    end: Instant,
    mut tick: impl FnMut() -> Result<()>,
) -> Result<()> {
    tick()?;
    for pin in &prepared.request.score_files {
        read(prepared.inputs.assets, pin, 1024 * 1024)?;
        tick()?;
    }
    for instruction in &prepared.request.instructions {
        read(prepared.inputs.assets, &instruction.source, 65536)?;
        read(prepared.inputs.assets, &instruction.authority, 65536)?;
        tick()?;
    }
    reading_bytes(&prepared.request, prepared.inputs)?;
    reading::check(
        prepared.inputs.reading,
        prepared.inputs.reading_directory,
        end.saturating_duration_since(Instant::now()),
    )?;
    tick()?;
    Ok(())
}
pub fn produce(
    prepared: &Prepared<'_>,
    output: &Path,
    mut control: producer::ProductionControl<impl FnMut() -> Result<()>>,
) -> Result<Summary> {
    let end = deadline(control.timeout)?;
    let mut tick = || {
        (control.cancellation)()?;
        ensure!(Instant::now() < end, "lesson deadline elapsed");
        Ok(())
    };
    recheck(prepared, end, &mut tick)?;
    let mut owner = producer::OutputOwner::reserve(output)?;
    producer::persist_file(&mut owner, "lesson.json", &prepared.index, &mut tick)?;
    owner.finish();
    Ok(summary(prepared))
}
pub fn check(prepared: &Prepared<'_>, output: &Path, timeout: Duration) -> Result<Summary> {
    let end = deadline(timeout)?;
    let tick = || {
        ensure!(Instant::now() < end, "lesson deadline elapsed");
        Ok(())
    };
    recheck(prepared, end, tick)?;
    let entries: Vec<_> = fs::read_dir(output)?
        .take(2)
        .collect::<std::io::Result<_>>()?;
    ensure!(
        entries.len() == 1 && entries[0].file_name() == "lesson.json",
        "lesson output namespace differs"
    );
    let pin = FileRef {
        path: "lesson.json".into(),
        sha256: sha(&prepared.index),
        bytes: prepared.index.len() as u64,
    };
    ensure!(
        read(&output.to_path_buf(), &pin, 1024 * 1024)? == prepared.index,
        "lesson index differs"
    );
    tick()?;
    Ok(summary(prepared))
}
fn summary(prepared: &Prepared<'_>) -> Summary {
    Summary {
        editions: prepared.request.edition_ids.len(),
        occurrences: prepared.request.occurrences.len(),
        anchors: prepared.request.anchors.len(),
        source_bindings_checked: true,
        musical_accuracy_verified: false,
        authority_verified: false,
        device_playback_checked: false,
        release_authorized: false,
    }
}
