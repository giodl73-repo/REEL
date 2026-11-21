//! Additive audio-edition declarations. Validation reads no media and grants no authority.
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{cue_relative::CueMarker, scene_delivery::FileRef};

pub mod checker;
pub mod compiler;
#[cfg(feature = "tig-history")]
pub mod history;
pub mod layers;
pub mod producer;

pub const SCHEMA: &str = "reel.spoken-edition.v1";
pub const VALIDATION_SCHEMA: &str = "reel.spoken-edition-validation.v1";
pub const MAX_DECLARATION_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema: String,
    pub id: String,
    pub sample_rate: u32,
    pub modules: Vec<Module>,
    pub performances: Vec<Performance>,
    pub editions: Vec<Edition>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub id: String,
    pub requires: Vec<String>,
    pub cues: Vec<Cue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cue {
    pub id: String,
    pub speaker_id: String,
    pub scripts: Vec<Script>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub id: String,
    pub language: String,
    pub text: String,
    pub sha256: String,
    pub authority: FileRef,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Performance {
    pub id: String,
    pub module_id: String,
    pub cue_id: String,
    pub language: String,
    pub script_id: String,
    pub script_sha256: String,
    pub observed_text_sha256: String,
    pub source: FileRef,
    pub alignment: FileRef,
    pub duration_samples: u64,
    pub markers: Vec<CueMarker>,
    pub protected: Vec<ProtectedRegion>,
    pub scope: ReuseScope,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedRegion {
    pub kind: ProtectedKind,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProtectedKind {
    Word,
    Breath,
    Tail,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReuseScope {
    pub kind: PerformanceKind,
    pub speaker_id: String,
    pub language: String,
    pub edition_ids: Vec<String>,
    pub evidence: FileRef,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PerformanceKind {
    EngineeringSignal,
    RecordedSpeech,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    pub id: String,
    pub language: String,
    pub audience: String,
    pub required_modules: Vec<String>,
    pub selections: Vec<Selection>,
    pub joins: Vec<Join>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub module_id: String,
    pub takes: Vec<TakeUse>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TakeUse {
    pub performance_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Join {
    pub from: String,
    pub to: String,
    pub added_pause_samples: u64,
    pub overlap_samples: u64,
}

/// Portable report intentionally contains no private source, text or identity fields.
#[derive(Debug, Serialize)]
pub struct Validation {
    pub schema: &'static str,
    pub modules: usize,
    pub performances: usize,
    pub editions: usize,
    pub media_verified: bool,
    pub authority_verified: bool,
}

pub fn load(path: &Path) -> Result<Document> {
    let mut bytes = Vec::new();
    File::open(path)
        .context("spoken declaration unavailable")?
        .take(MAX_DECLARATION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .context("spoken declaration read failed")?;
    parse(&bytes)
}

pub fn parse(bytes: &[u8]) -> Result<Document> {
    if bytes.len() > MAX_DECLARATION_BYTES {
        bail!("spoken declaration exceeds budget");
    }
    // Do not include Serde's input-bearing error in portable CLI diagnostics.
    let document: Document = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("spoken declaration is not strict JSON"))?;
    validate(&document)?;
    Ok(document)
}

fn require(ok: bool, message: &'static str) -> Result<()> {
    if !ok {
        bail!(message);
    }
    Ok(())
}

fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_./".contains(&c))
}

fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn pin(value: &FileRef) -> Result<()> {
    require(
        hash(&value.sha256) && value.bytes > 0 && value.bytes <= 16 * 1024 * 1024,
        "spoken reference pin invalid",
    )?;
    crate::scene_delivery::control_name(&value.path)
        .map_err(|_| anyhow::anyhow!("spoken reference name invalid"))?;
    Ok(())
}

fn unique<'a>(mut values: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    values.all(|s| id(s) && seen.insert(s))
}

pub fn validate(doc: &Document) -> Result<Validation> {
    require(
        doc.schema == SCHEMA && id(&doc.id),
        "spoken schema or identity invalid",
    )?;
    require(doc.sample_rate == 48_000, "spoken sample rate unsupported")?;
    require(
        (1..=16).contains(&doc.modules.len())
            && (1..=128).contains(&doc.performances.len())
            && (1..=16).contains(&doc.editions.len())
            && doc.modules.iter().map(|m| m.cues.len()).sum::<usize>() <= 64,
        "spoken declaration count exceeds budget",
    )?;
    require(
        unique(doc.modules.iter().map(|m| m.id.as_str()))
            && unique(doc.performances.iter().map(|p| p.id.as_str()))
            && unique(doc.editions.iter().map(|e| e.id.as_str())),
        "spoken duplicate or invalid identity",
    )?;
    let mut languages = BTreeSet::new();
    for module in &doc.modules {
        require(
            !module.cues.is_empty()
                && unique(module.cues.iter().map(|c| c.id.as_str()))
                && unique(module.requires.iter().map(String::as_str))
                && module
                    .requires
                    .iter()
                    .all(|r| r != &module.id && doc.modules.iter().any(|m| &m.id == r)),
            "spoken module cues or context invalid",
        )?;
        for cue in &module.cues {
            require(
                id(&cue.speaker_id)
                    && !cue.scripts.is_empty()
                    && cue.scripts.len() <= 8
                    && unique(cue.scripts.iter().map(|s| s.id.as_str())),
                "spoken cue scripts or speaker invalid",
            )?;
            for script in &cue.scripts {
                languages.insert(script.language.as_str());
                require(
                    script.language.len() == 2
                        && script.language.bytes().all(|c| c.is_ascii_lowercase())
                        && !script.text.trim().is_empty()
                        && script.text.len() <= 8192
                        && Sha256::digest(script.text.as_bytes())
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>()
                            == script.sha256,
                    "spoken script identity invalid",
                )?;
                pin(&script.authority)?;
            }
        }
    }
    require(languages.len() <= 8, "spoken language count exceeds budget")?;
    for module in &doc.modules {
        let mut pending = module.requires.iter().collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        while let Some(next) = pending.pop() {
            require(next != &module.id, "spoken context dependency cycle")?;
            if seen.insert(next) {
                let dependency = doc
                    .modules
                    .iter()
                    .find(|m| &m.id == next)
                    .context("spoken required module missing")?;
                pending.extend(dependency.requires.iter());
            }
        }
    }
    for p in &doc.performances {
        let cue = doc
            .modules
            .iter()
            .find(|m| m.id == p.module_id)
            .and_then(|m| m.cues.iter().find(|c| c.id == p.cue_id))
            .context("spoken performance cue missing")?;
        require(
            cue.scripts.iter().any(|s| {
                s.id == p.script_id && s.language == p.language && s.sha256 == p.script_sha256
            }) && hash(&p.observed_text_sha256)
                && (1..=720_000).contains(&p.duration_samples),
            "spoken performance script or duration invalid",
        )?;
        pin(&p.source)?;
        pin(&p.alignment)?;
        pin(&p.scope.evidence)?;
        require(
            p.scope.speaker_id == cue.speaker_id
                && p.scope.language == p.language
                && !p.scope.edition_ids.is_empty()
                && unique(p.scope.edition_ids.iter().map(String::as_str))
                && p.scope.edition_ids.iter().all(|id| {
                    doc.editions
                        .iter()
                        .any(|e| &e.id == id && e.language == p.language)
                }),
            "spoken performance reuse scope invalid",
        )?;
        require(
            p.markers.len() <= 1024
                && p.protected.len() <= 1024
                && unique(p.markers.iter().map(|m| m.id.as_str())),
            "spoken marker count or identity invalid",
        )?;
        for marker in &p.markers {
            require(
                marker.start_sample < marker.end_sample && marker.end_sample <= p.duration_samples,
                "spoken marker interval invalid",
            )?;
        }
        for region in &p.protected {
            require(
                region.start_sample < region.end_sample && region.end_sample <= p.duration_samples,
                "spoken protected interval invalid",
            )?;
        }
    }
    for edition in &doc.editions {
        require(
            id(&edition.audience)
                && languages.contains(edition.language.as_str())
                && !edition.selections.is_empty()
                && unique(edition.required_modules.iter().map(String::as_str))
                && edition
                    .required_modules
                    .iter()
                    .all(|id| edition.selections.iter().any(|s| &s.module_id == id))
                && unique(edition.selections.iter().map(|s| s.module_id.as_str())),
            "spoken edition selection invalid",
        )?;
        let mut prior = BTreeSet::new();
        let mut uses = Vec::new();
        for selection in &edition.selections {
            let module = doc
                .modules
                .iter()
                .find(|m| m.id == selection.module_id)
                .context("spoken selected module missing")?;
            require(
                module.requires.iter().all(|r| prior.contains(r)),
                "spoken required context missing",
            )?;
            prior.insert(&module.id);
            require(
                selection.takes.len() == module.cues.len(),
                "spoken selected cue count invalid",
            )?;
            for (take, cue) in selection.takes.iter().zip(&module.cues) {
                let p = doc
                    .performances
                    .iter()
                    .find(|p| p.id == take.performance_id)
                    .context("spoken selected performance missing")?;
                require(
                    p.module_id == module.id
                        && p.cue_id == cue.id
                        && p.language == edition.language
                        && p.scope.edition_ids.contains(&edition.id),
                    "spoken selected performance scope invalid",
                )?;
                require(
                    take.start_sample < take.end_sample
                        && take.end_sample <= p.duration_samples
                        && p.protected.iter().all(|r| {
                            take.start_sample <= r.start_sample && r.end_sample <= take.end_sample
                        }),
                    "spoken selected trim crosses protected interval",
                )?;
                uses.push((take, p));
            }
        }
        require(
            edition.joins.len() == uses.len() - 1,
            "spoken adjacency count invalid",
        )?;
        for (join, pair) in edition.joins.iter().zip(uses.windows(2)) {
            let (a, pa) = pair[0];
            let (b, pb) = pair[1];
            require(
                join.from == pa.id
                    && join.to == pb.id
                    && join.added_pause_samples <= 720_000
                    && (join.overlap_samples == 0 || join.added_pause_samples == 0),
                "spoken handoff policy invalid",
            )?;
            let overlap = join.overlap_samples;
            require(
                overlap < a.end_sample - a.start_sample && overlap < b.end_sample - b.start_sample,
                "spoken overlap exceeds selected interval",
            )?;
            if overlap > 0 {
                require(
                    pa.protected
                        .iter()
                        .all(|r| r.end_sample <= a.end_sample - overlap)
                        && pb
                            .protected
                            .iter()
                            .all(|r| r.start_sample >= b.start_sample + overlap),
                    "spoken overlap crosses protected interval",
                )?;
            }
        }
    }
    Ok(Validation {
        schema: VALIDATION_SCHEMA,
        modules: doc.modules.len(),
        performances: doc.performances.len(),
        editions: doc.editions.len(),
        media_verified: false,
        authority_verified: false,
    })
}
