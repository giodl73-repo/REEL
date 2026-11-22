//! Portable review-map wire contracts. No media, storage, viewing or authority.
use super::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub const MAP_SCHEMA: &str = "reel.film-cel-review-map.v1";
pub const RECEIPT_SCHEMA: &str = "reel.film-cel-review-receipt.v1";
pub const MAX_MAP_BYTES: usize = 4 * 1024 * 1024;
macro_rules! ensure {
    ($condition:expr, $code:literal) => {
        if !$condition {
            return Err(Error {
                kind: ErrorKind::Invalid,
                code: $code,
            });
        }
    };
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub attachment_id: String,
    pub story_id: String,
    pub cel_id: String,
    pub semantic_id: String,
    pub selected_picture: SourceFile,
    pub start_frame: u64,
    pub end_frame: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewMap {
    pub schema: String,
    pub request_sha256: String,
    pub project: String,
    pub source_commit: String,
    pub language: String,
    pub source_film: SourceFile,
    pub delivery_receipt: SourceFile,
    pub job: SourceFile,
    pub frame_count: u64,
    pub fps_numerator: u64,
    pub fps_denominator: u64,
    pub width: u32,
    pub height: u32,
    pub targets: Vec<Target>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub request_sha256: String,
    pub source_clocks_sha256: String,
    pub output_clocks_sha256: String,
    pub frame_count: u64,
    pub audio_packet_count: u64,
    pub outputs: BTreeMap<String, SourceFile>,
    pub publication: String,
    pub visual_reviewed: bool,
    pub approvals_verified: bool,
    pub native_retention_verified: bool,
}
fn id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 128
        && v != "."
        && v != ".."
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
pub fn sha(v: &str) -> bool {
    v.len() == 64
        && v.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn file(v: &SourceFile) -> bool {
    producer_path(v).is_some() && sha(&v.sha256) && v.bytes > 0
}
fn producer_path(v: &SourceFile) -> Option<String> {
    let path = v.path.to_str()?;
    if path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || path.contains(':')
    {
        return None;
    }
    // Compare producer namespaces consistently when a Windows-authored map is
    // inspected elsewhere. Keep the original spelling in the immutable wire.
    let normalized = path.replace('\\', "/");
    if normalized
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return None;
    }
    Some(normalized)
}
/// Validate typed shape/coverage. Readers/writers enforce MAX_MAP_BYTES on bytes.
pub fn check_map(map: &ReviewMap) -> Result<()> {
    ensure!(
        map.schema == MAP_SCHEMA
            && sha(&map.request_sha256)
            && id(&map.project)
            && id(&map.language)
            && [40, 64].contains(&map.source_commit.len())
            && map
                .source_commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && !map.source_commit.bytes().all(|b| b == b'0')
            && map.frame_count > 0
            && map.frame_count <= 1_000_000
            && map.fps_numerator > 0
            && map.fps_denominator > 0
            && (160..=7680).contains(&map.width)
            && (64..=4320).contains(&map.height)
            && !map.targets.is_empty()
            && map.targets.len() <= 2048
            && file(&map.source_film)
            && file(&map.delivery_receipt)
            && file(&map.job),
        "film-review-map-shape"
    );
    ensure!(
        producer_path(&map.source_film)
            == Some(
                match producer_path(&map.delivery_receipt)
                    .unwrap()
                    .rsplit_once('/')
                {
                    Some((parent, _)) => format!("{parent}/review.mp4"),
                    None => "review.mp4".into(),
                }
            ),
        "film-review-source-namespace"
    );
    let mut ids = BTreeSet::new();
    let mut cursor = 0;
    for target in &map.targets {
        ensure!(
            id(&target.attachment_id)
                && id(&target.story_id)
                && id(&target.cel_id)
                && id(&target.semantic_id)
                && ids.insert(&target.attachment_id)
                && file(&target.selected_picture)
                && target.start_frame == cursor
                && target.end_frame > cursor
                && target.end_frame <= map.frame_count,
            "film-review-target"
        );
        cursor = target.end_frame;
    }
    ensure!(cursor == map.frame_count, "film-review-coverage");
    Ok(())
}
