//! Explicit, exact-text lyric underlay proposals. No recording/cache discovery.
use crate::{
    hash::sha256_bytes,
    model::{self, LyricUnderlay, MusicModel, PartRole},
    model_editor::{self as io, SAFE},
    nonempty, source,
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{fs, path::Path};
const EDIT: &str = "reel.music-underlay-edit.v1";
const LYRIC_LIMIT: usize = 128 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub schema: String,
    pub expected_model_sha256: String,
    pub expected_lyric_sha256: String,
    pub lyric_layer_id: String,
    pub candidate_model_id: String,
    pub underlay: Vec<LyricUnderlay>,
    pub rationale: String,
}
struct Captured {
    model: MusicModel,
    model_sha: String,
    lyric_sha: String,
    text: String,
    layer_id: String,
}
fn bound(rows: &[LyricUnderlay]) -> Result<()> {
    if rows.len() > 1000 || rows.iter().map(|row| row.note_ids.len()).sum::<usize>() > 1000 {
        bail!("Underlay supports at most 1000 spans and 1000 note references");
    }
    for row in rows {
        io::identifier(&row.id)?;
        if row.text_start_byte > SAFE || row.text_end_byte > SAFE {
            bail!("Underlay offsets exceed exact browser integer range");
        }
        for id in &row.note_ids {
            io::identifier(id)?;
        }
    }
    Ok(())
}
fn capture(path: &Path, lyric: &Path) -> Result<Captured> {
    let snapshot = capture_bytes(&io::read(path)?, &io::read_bounded(lyric, LYRIC_LIMIT)?)?;
    let layer = snapshot
        .model
        .lyric_layers
        .iter()
        .find(|layer| layer.id == snapshot.layer_id)
        .unwrap();
    if source::resolve(path, &layer.path).canonicalize()? != lyric.canonicalize()? {
        bail!("Explicit lyric file must match the native selected layer path");
    }
    Ok(snapshot)
}
fn capture_bytes(raw: &[u8], lyric: &[u8]) -> Result<Captured> {
    let (model, model_sha) = io::intake_bytes(raw)?;
    if lyric.len() > LYRIC_LIMIT {
        bail!("Lyric text exceeds authoring budget");
    }
    let sheet = model
        .lead_sheet
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Declare a native lead sheet before underlay editing"))?;
    let layer_id = sheet
        .lyric_layer_id
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Declare the native lyric layer before underlay editing"))?
        .clone();
    io::identifier(&layer_id)?;
    let part = model
        .parts
        .iter()
        .find(|part| part.id == sheet.melody_part_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown native melody part"))?;
    if !matches!(part.role, PartRole::Melody | PartRole::Vocal) {
        bail!("Underlay requires a melody or vocal part");
    }
    let mut layers = model
        .lyric_layers
        .iter()
        .filter(|layer| layer.id == layer_id);
    let layer = layers
        .next()
        .ok_or_else(|| anyhow::anyhow!("Unknown native lyric layer"))?;
    if layers.next().is_some() {
        bail!("Ambiguous native lyric layer");
    }
    let lyric_sha = sha256_bytes(lyric);
    if !lyric_sha.eq_ignore_ascii_case(&layer.sha256) {
        bail!("Selected lyric byte hash does not match");
    }
    let text = String::from_utf8(lyric.to_vec())?;
    bound(&sheet.underlay)?;
    Ok(Captured {
        model,
        model_sha,
        lyric_sha,
        text,
        layer_id,
    })
}
fn edit_for(snapshot: &Captured) -> Edit {
    Edit {
        schema: EDIT.into(),
        expected_model_sha256: snapshot.model_sha.clone(),
        expected_lyric_sha256: snapshot.lyric_sha.clone(),
        lyric_layer_id: snapshot.layer_id.clone(),
        candidate_model_id: format!("underlay-proposal-{}", &snapshot.model_sha[..16]),
        underlay: snapshot.model.lead_sheet.as_ref().unwrap().underlay.clone(),
        rationale: String::new(),
    }
}
pub fn template(path: &Path, lyric: &Path) -> Result<Edit> {
    Ok(edit_for(&capture(path, lyric)?))
}
pub fn export(path: &Path, lyric: &Path, output: &Path) -> Result<usize> {
    let snapshot = capture(path, lyric)?;
    export_snapshot(snapshot, output)
}
/// Export exact captured model and lyric bytes without filesystem dependency lookup.
pub fn export_bytes(raw: &[u8], lyric: &[u8], output: &Path) -> Result<usize> {
    export_snapshot(capture_bytes(raw, lyric)?, output)
}
fn export_snapshot(snapshot: Captured, output: &Path) -> Result<usize> {
    let request = edit_for(&snapshot);
    let sheet = snapshot.model.lead_sheet.as_ref().unwrap();
    let part = snapshot
        .model
        .parts
        .iter()
        .find(|part| part.id == sheet.melody_part_id)
        .unwrap();
    let layer = snapshot
        .model
        .lyric_layers
        .iter()
        .find(|layer| layer.id == snapshot.layer_id)
        .unwrap();
    let session = json!({"schema":"reel.music-underlay-session.v1","model_id":snapshot.model.model_id,
        "edit_request":request,"lyric_text":snapshot.text,"language":layer.language,"lyric_kind":layer.kind,
        "melody_part_id":part.id,"notes":part.notes.iter().map(|note|json!({"id":note.id,"start_tick":note.start_tick,"duration_ticks":note.duration_ticks,"midi_note":note.midi_note})).collect::<Vec<_>>(),
        "lyric_bytes_verified":true,"audio_resolved":false,"native_validation":"not-run"});
    let bytes = io::encode(&session)?;
    fs::create_dir(output)?;
    io::write_new(&output.join("session.json"), &bytes)?;
    io::write_new(
        &output.join("index.html"),
        include_bytes!("underlay_editor.html"),
    )?;
    io::write_new(
        &output.join("editor.js"),
        include_bytes!("underlay_editor.js"),
    )?;
    Ok(request.underlay.len())
}
pub fn apply(path: &Path, lyric: &Path, request: &Path, output: &Path) -> Result<usize> {
    let snapshot = capture(path, lyric)?;
    io::check_destination(path, output)?;
    let model_sha = snapshot.model_sha.clone();
    let lyric_sha = snapshot.lyric_sha.clone();
    let (candidate, count) = propose_snapshot(snapshot, &io::read(request)?)?;
    if sha256_bytes(&io::read_bounded(lyric, LYRIC_LIMIT)?) != lyric_sha {
        bail!("Lyric bytes changed during application");
    }
    io::publish_bytes(path, &model_sha, output, &candidate)?;
    Ok(count)
}
/// Return an unreviewed model; dependency names are preserved, not relocated.
pub fn propose_bytes(raw: &[u8], lyric: &[u8], request: &[u8]) -> Result<(Vec<u8>, usize)> {
    propose_snapshot(capture_bytes(raw, lyric)?, request)
}
fn propose_snapshot(mut snapshot: Captured, request: &[u8]) -> Result<(Vec<u8>, usize)> {
    if request.len() > 2 * 1024 * 1024 {
        bail!("Underlay request exceeds authoring budget");
    }
    let request: Edit = serde_json::from_slice(request)?;
    if request.schema != EDIT
        || request.expected_model_sha256 != snapshot.model_sha
        || request.expected_lyric_sha256 != snapshot.lyric_sha
        || request.lyric_layer_id != snapshot.layer_id
    {
        bail!("Stale, wrong-layer or unsupported underlay edit");
    }
    io::identifier(&request.candidate_model_id)?;
    if request.candidate_model_id == snapshot.model.model_id {
        bail!("Proposal needs a new model identity");
    }
    bound(&request.underlay)?;
    if request.rationale.len() > 4096 {
        bail!("Underlay rationale exceeds budget");
    }
    let sheet = snapshot.model.lead_sheet.as_mut().unwrap();
    if request.underlay != sheet.underlay {
        nonempty("changed underlay rationale", &request.rationale)?;
    }
    sheet.underlay = request.underlay;
    model::validate_lead_sheet_underlay(&snapshot.model, Some(&snapshot.text))?;
    let count = snapshot.model.lead_sheet.as_ref().unwrap().underlay.len();
    snapshot.model.model_id = request.candidate_model_id;
    io::unreviewed(&mut snapshot.model);
    let role = "lyrics-vocal-adaptation-editor";
    if !snapshot
        .model
        .review
        .required_roles
        .iter()
        .any(|item| item == role)
    {
        snapshot.model.review.required_roles.push(role.into());
    }
    Ok((io::encode(&snapshot.model)?, count))
}
