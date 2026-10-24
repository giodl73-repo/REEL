//! Shared bounded model-editor I/O. No dependency discovery or media resolution.
use crate::{
    hash::sha256_bytes,
    model::{self, MusicModel},
    nonempty,
};
use anyhow::{Result, bail};
use serde::Serialize;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const LIMIT: usize = 2 * 1024 * 1024;
pub(crate) const SAFE: u64 = 9_007_199_254_740_991;
pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    read_bounded(path, LIMIT)
}
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if reparse || !metadata.is_file() || metadata.len() > limit as u64 {
        bail!("Editor input requires a bounded regular non-reparse file");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        bail!("Editor input exceeds control budget");
    }
    Ok(bytes)
}
pub(crate) fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > LIMIT {
        bail!("Editor output exceeds control budget");
    }
    Ok(bytes)
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}
pub(crate) fn intake(path: &Path) -> Result<(MusicModel, String)> {
    let raw = read(path)?;
    let model: MusicModel = serde_yaml::from_slice(&raw)?;
    layout(&model)?;
    Ok((model, sha256_bytes(&raw)))
}
pub(crate) fn identifier(value: &str) -> Result<()> {
    nonempty("editor identifier", value)?;
    if value.len() > 256 {
        bail!("Editor identifier exceeds budget");
    }
    Ok(())
}
pub(crate) fn layout(model: &MusicModel) -> Result<()> {
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

fn parent(path: &Path) -> Result<PathBuf> {
    Ok(path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?)
}
pub(crate) fn check_destination(source: &Path, output: &Path) -> Result<()> {
    if parent(source)? != parent(output)? {
        bail!("Write beside the source to preserve native relative references");
    }
    Ok(())
}
pub(crate) fn unreviewed(model: &mut MusicModel) {
    model.authority.status = "not-reviewed".into();
    model.authority.decision_refs.clear();
    model.review.status = "not-reviewed".into();
    model.review.decision_refs.clear();
}
pub(crate) fn publish(source: &Path, hash: &str, output: &Path, model: &MusicModel) -> Result<()> {
    check_destination(source, output)?;
    let bytes = encode(model)?;
    if sha256_bytes(&read(source)?) != hash {
        bail!("Source model changed during application");
    }
    let mut candidate = tempfile::NamedTempFile::new_in(parent(output)?)?;
    candidate.write_all(&bytes)?;
    candidate.persist_noclobber(output)?;
    Ok(())
}
