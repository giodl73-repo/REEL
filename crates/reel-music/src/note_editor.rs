//! Bounded, offline note proposals over the native model. No dependency/media I/O.
use crate::{
    hash::sha256_bytes,
    model::{self, MusicModel, ProvenanceState},
    nonempty,
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::Path,
};

const LIMIT: usize = 2 * 1024 * 1024;
const SAFE: u64 = 9_007_199_254_740_991;
const EDIT: &str = "reel.music-note-edit.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub part_id: String,
    pub note_id: String,
    pub midi_note: u8,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub rationale: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub schema: String,
    pub expected_model_sha256: String,
    pub candidate_model_id: String,
    pub rows: Vec<Row>,
}

fn read(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if reparse || !metadata.is_file() || metadata.len() > LIMIT as u64 {
        bail!("Editor input requires a bounded regular non-reparse file");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        bail!("Editor input exceeds control budget");
    }
    Ok(bytes)
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > LIMIT {
        bail!("Editor output exceeds control budget");
    }
    Ok(bytes)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}
fn intake(path: &Path) -> Result<(MusicModel, String)> {
    let raw = read(path)?;
    let model: MusicModel = serde_yaml::from_slice(&raw)?;
    layout(&model)?;
    Ok((model, sha256_bytes(&raw)))
}
fn identifier(value: &str) -> Result<()> {
    nonempty("editor identifier", value)?;
    if value.len() > 256 {
        bail!("Editor identifier exceeds budget");
    }
    Ok(())
}
fn layout(model: &MusicModel) -> Result<()> {
    if model.schema != model::SCHEMA || model.duration_ticks == 0 || model.duration_ticks > SAFE {
        bail!("Unsupported native model schema or tick clock");
    }
    identifier(&model.model_id)?;
    model.musical_timebase.validate()?;
    model::validate_note_layout(model)?;
    let count: usize = model.parts.iter().map(|part| part.notes.len()).sum();
    if model.parts.len() > 64 || count > 1000 {
        bail!("Editor supports at most 64 parts and 1000 notes");
    }
    for part in &model.parts {
        identifier(&part.id)?;
        let mut ends = std::collections::BTreeMap::new();
        for note in &part.notes {
            identifier(&note.id)?;
            if note.voice > 16
                || ends
                    .get(&note.voice)
                    .is_some_and(|end| *end > note.start_tick)
            {
                bail!("Editor requires MIDI voices 1–16 without same-voice overlaps");
            }
            ends.insert(note.voice, note.start_tick + note.duration_ticks);
        }
    }
    Ok(())
}
pub fn template(path: &Path) -> Result<Edit> {
    let (model, hash) = intake(path)?;
    edit_for(&model, hash)
}
fn edit_for(model: &MusicModel, hash: String) -> Result<Edit> {
    let candidate_model_id = format!("note-proposal-{}", &hash[..16]);
    identifier(&candidate_model_id)?;
    Ok(Edit {
        schema: EDIT.into(),
        expected_model_sha256: hash,
        candidate_model_id,
        rows: model
            .parts
            .iter()
            .flat_map(|part| {
                part.notes.iter().map(|note| Row {
                    part_id: part.id.clone(),
                    note_id: note.id.clone(),
                    midi_note: note.midi_note,
                    start_tick: note.start_tick,
                    duration_ticks: note.duration_ticks,
                    rationale: String::new(),
                })
            })
            .collect(),
    })
}
/// Export declared note metadata only. Native project validation is not run.
pub fn export(path: &Path, output: &Path) -> Result<usize> {
    let (model, hash) = intake(path)?;
    // Build from the same captured model, never reopen a mutable source for rows.
    let request = edit_for(&model, hash)?;
    let session = json!({"schema":"reel.music-note-session.v1", "model_id":model.model_id,
        "edit_request":request, "duration_ticks":model.duration_ticks,
        "pulses_per_quarter":model.musical_timebase.pulses_per_quarter,
        "parts":model.parts.iter().map(|part|json!({"id":part.id,"name":part.name,"role":part.role,
            "notes":part.notes.iter().map(|note|json!({"id":note.id,"voice":note.voice})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "lead_sheet":model.lead_sheet, "media_resolved":false,"native_validation":"not-run"});
    let bytes = encode(&session)?;
    fs::create_dir(output)?;
    write_new(&output.join("session.json"), &bytes)?;
    write_new(
        &output.join("index.html"),
        include_bytes!("note_editor.html"),
    )?;
    write_new(&output.join("editor.js"), include_bytes!("note_editor.js"))?;
    Ok(request.rows.len())
}
/// Write a new unreviewed native proposal beside its source; never resolve media.
pub fn apply(path: &Path, request_path: &Path, output: &Path) -> Result<usize> {
    let (mut model, hash) = intake(path)?;
    let request: Edit = serde_json::from_slice(&read(request_path)?)?;
    if request.schema != EDIT || request.expected_model_sha256 != hash {
        bail!("Stale or unsupported note edit");
    }
    identifier(&request.candidate_model_id)?;
    if request.candidate_model_id == model.model_id {
        bail!("Proposal needs a new model identity");
    }
    let parent = |path: &Path| {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()
    };
    if parent(path)? != parent(output)? {
        bail!("Write beside the source to preserve native relative references");
    }
    let mut seen = BTreeSet::new();
    let mut changed = 0;
    let count: usize = model.parts.iter().map(|part| part.notes.len()).sum();
    if request.rows.len() != count {
        bail!("Every original note is required exactly once");
    }
    for row in &request.rows {
        if !seen.insert((&row.part_id, &row.note_id)) {
            bail!("Duplicate note edit");
        }
        if row.rationale.len() > 4096 {
            bail!("Note rationale exceeds budget");
        }
        let note = model
            .parts
            .iter_mut()
            .find(|part| part.id == row.part_id)
            .and_then(|part| part.notes.iter_mut().find(|note| note.id == row.note_id))
            .ok_or_else(|| anyhow::anyhow!("Unknown part or note identity"))?;
        if (note.midi_note, note.start_tick, note.duration_ticks)
            != (row.midi_note, row.start_tick, row.duration_ticks)
        {
            nonempty("changed note rationale", &row.rationale)?;
            if note.provenance.evidence_refs.is_empty() {
                bail!("An inferred proposal needs existing evidence references");
            }
            note.midi_note = row.midi_note;
            note.start_tick = row.start_tick;
            note.duration_ticks = row.duration_ticks;
            note.provenance.state = ProvenanceState::Inferred;
            note.provenance.correction_ref = None;
            note.provenance.rationale = format!("Unreviewed note proposal: {}", row.rationale);
            changed += 1;
        }
    }
    layout(&model)?;
    model.model_id = request.candidate_model_id;
    model.authority.status = "not-reviewed".into();
    model.authority.decision_refs.clear();
    model.review.status = "not-reviewed".into();
    model.review.decision_refs.clear();
    let bytes = encode(&model)?;
    // Recheck the source identity before publishing the new, separate file.
    if sha256_bytes(&read(path)?) != hash {
        bail!("Source model changed during application");
    }
    let mut candidate = tempfile::NamedTempFile::new_in(parent(output)?)?;
    candidate.write_all(&bytes)?;
    candidate.persist_noclobber(output)?;
    Ok(changed)
}
