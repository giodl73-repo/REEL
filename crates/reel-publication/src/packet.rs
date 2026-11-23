//! Deterministic bounded byte carrier. No extraction, execution or custody.
use crate::comic::{object, portable};
use crate::comic_source::Object;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const MEDIA_TYPE: &str = "application/vnd.reel.editable-packet.v1";
const MAGIC: &[u8; 8] = b"REELPKT1";
const MAX_BYTES: usize = 128 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema: String,
    files: BTreeMap<String, Object>,
}
fn names<'a>(files: impl Iterator<Item = &'a String>) -> Result<()> {
    let mut folded = BTreeSet::new();
    for name in files {
        ensure!(
            portable(name)
                && name.split('/').count() <= 16
                && name.split('/').all(|part| {
                    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
                    !part.ends_with('.')
                        && !matches!(
                            stem.as_str(),
                            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                        )
                        && !(stem.len() == 4
                            && (stem.starts_with("COM") || stem.starts_with("LPT"))
                            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
                })
                && folded.insert(name.to_ascii_lowercase()),
            "packet path/case refused"
        );
    }
    ensure!(
        (1..=256).contains(&folded.len()),
        "packet file count refused"
    );
    for path in &folded {
        for (index, _) in path.match_indices('/') {
            ensure!(
                !folded.contains(&path[..index]),
                "packet file/directory alias refused"
            );
        }
    }
    Ok(())
}
pub fn encode(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    names(files.keys())?;
    let index = Index {
        schema: "reel.editable-packet.v1".into(),
        files: files
            .iter()
            .map(|(name, raw)| (name.clone(), object(raw)))
            .collect(),
    };
    let header = serde_json::to_vec(&index)?;
    ensure!(
        header.len() <= crate::MAX_CONTROL_BYTES,
        "packet header bound"
    );
    let total = files
        .values()
        .try_fold(12 + header.len(), |n, raw| n.checked_add(raw.len()))
        .context("packet byte overflow")?;
    ensure!(total <= MAX_BYTES, "packet byte bound");
    let mut raw = Vec::with_capacity(total);
    raw.extend_from_slice(MAGIC);
    raw.extend_from_slice(&(header.len() as u32).to_le_bytes());
    raw.extend(header);
    for bytes in files.values() {
        raw.extend_from_slice(bytes);
    }
    Ok(raw)
}
pub fn decode(raw: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    ensure!(
        (12..=MAX_BYTES).contains(&raw.len()) && &raw[..8] == MAGIC,
        "packet framing refused"
    );
    let size = u32::from_le_bytes(raw[8..12].try_into()?) as usize;
    ensure!(
        size <= crate::MAX_CONTROL_BYTES && size <= raw.len() - 12,
        "packet header bound"
    );
    let index: Index = serde_json::from_slice(&raw[12..12 + size])?;
    ensure!(
        index.schema == "reel.editable-packet.v1",
        "packet schema refused"
    );
    names(index.files.keys())?;
    let mut offset = 12 + size;
    let mut files = BTreeMap::new();
    for (name, expected) in index.files {
        let bytes = usize::try_from(expected.bytes)?;
        let end = offset
            .checked_add(bytes)
            .context("packet length overflow")?;
        ensure!(end <= raw.len(), "packet truncated");
        let value = &raw[offset..end];
        ensure!(object(value) == expected, "packet content differs");
        files.insert(name, value.to_vec());
        offset = end;
    }
    ensure!(offset == raw.len(), "packet trailing bytes refused");
    // Enforce the canonical index representation, including duplicate-key refusal.
    ensure!(encode(&files)? == raw, "noncanonical packet refused");
    Ok(files)
}
