//! Publication assembly contracts, not storage publication, creative approval,
//! press certification or remote delivery. Renderers remain separate adapters.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
#[cfg(feature = "comic")]
pub mod comic;
#[cfg(feature = "comic")]
pub mod comic_issue;
pub mod comic_issue_source;
pub mod comic_source;

pub const PUBLICATION_SCHEMA: &str = "reel.publication.v1";
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUTS: usize = 256;
const MAX_EDITIONS: usize = 32;
const MAX_PAGES: usize = 512;
const MAX_PAGE_ITEMS: usize = 256;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    Invalid,
    Missing,
    Conflict,
    Budget,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub code: &'static str,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.code)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn fail<T>(kind: ErrorKind, code: &'static str) -> Result<T> {
    Err(Error { kind, code })
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub logical_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    pub media_type: String,
    pub max_bytes: u64,
    pub required: bool,
}
pub type OutputContracts = BTreeMap<String, OutputContract>;
pub type ArtifactSet = BTreeMap<String, Artifact>;

pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn media(value: &str) -> bool {
    value.len() <= 128
        && value.split('/').count() == 2
        && value.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
        })
}
pub fn check_artifact(value: &Artifact) -> Result<()> {
    if value.logical_id.len() > 256
        || !value.logical_id.split('/').all(valid_id)
        || value.sha256.len() != 64
        || !value
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || value.bytes == 0
        || !media(&value.media_type)
    {
        return fail(ErrorKind::Invalid, "artifact-descriptor");
    }
    Ok(())
}
pub fn check_contracts(contracts: &OutputContracts) -> Result<()> {
    if contracts.is_empty() || contracts.len() > MAX_OUTPUTS {
        return fail(ErrorKind::Budget, "output-count");
    }
    let mut total = 0u64;
    for (name, output) in contracts {
        if !valid_id(name) || !media(&output.media_type) || output.max_bytes == 0 {
            return fail(ErrorKind::Invalid, "output-contract");
        }
        total = total.checked_add(output.max_bytes).ok_or(Error {
            kind: ErrorKind::Budget,
            code: "output-total-overflow",
        })?;
    }
    if !contracts.values().any(|o| o.required) {
        return fail(ErrorKind::Invalid, "required-output-missing");
    }
    Ok(())
}

/// Descriptor checks only: no payload reads or retained-ownership inference.
pub fn check_artifact_set(artifacts: &ArtifactSet) -> Result<()> {
    if artifacts.is_empty() || artifacts.len() > MAX_OUTPUTS {
        return fail(ErrorKind::Budget, "artifact-count");
    }
    let mut logical = BTreeMap::new();
    for (name, value) in artifacts {
        if !valid_id(name) {
            return fail(ErrorKind::Invalid, "output-name");
        }
        check_artifact(value)?;
        if let Some(prior) = logical.insert(&value.logical_id, value) {
            if prior != value {
                return fail(ErrorKind::Conflict, "artifact-identity-conflict");
            }
        }
    }
    Ok(())
}
pub fn check_outputs(contracts: &OutputContracts, artifacts: &ArtifactSet) -> Result<()> {
    check_contracts(contracts)?;
    check_artifact_set(artifacts)?;
    for (name, contract) in contracts {
        if contract.required && !artifacts.contains_key(name) {
            return fail(ErrorKind::Missing, "required-output");
        }
    }
    for (name, value) in artifacts {
        let contract = contracts.get(name).ok_or(Error {
            kind: ErrorKind::Invalid,
            code: "undeclared-output",
        })?;
        if value.media_type != contract.media_type {
            return fail(ErrorKind::Invalid, "output-media-type");
        }
        if value.bytes > contract.max_bytes {
            return fail(ErrorKind::Budget, "output-bytes");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub schema: String,
    pub id: String,
    pub editions: Vec<Edition>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    pub id: String,
    pub language: String,
    pub content: Content,
    pub outputs: OutputContracts,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "format", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Content {
    /// Existing REEL production/composition source owns all audiovisual timing.
    Film { composition: Artifact },
    /// Page source adapters own geometry, lettering, layer closure and rendering.
    Comic {
        reading_direction: ReadingDirection,
        pages: Vec<ComicPage>,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReadingDirection {
    LeftToRight,
    RightToLeft,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PageKind {
    Story,
    Cover,
    Matter,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComicPage {
    pub id: String,
    pub kind: PageKind,
    pub source: Artifact,
    /// Ordered stable IDs, never an automatically selected grid or panel count.
    pub panel_order: Vec<String>,
    /// Reading order of stable utterance IDs; empty is valid for a silent page.
    pub utterance_order: Vec<String>,
    /// When present, exactly two adjacent pages must name the same spread.
    pub spread_id: Option<String>,
}
fn unique_ids(ids: &[String]) -> bool {
    ids.len() <= MAX_PAGE_ITEMS
        && ids.iter().all(|id| valid_id(id))
        && ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
}
pub fn check_publication(publication: &Publication) -> Result<()> {
    let bytes = serde_json::to_vec(publication).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "publication-encoding",
    })?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "publication-bytes");
    }
    if publication.schema != PUBLICATION_SCHEMA
        || !valid_id(&publication.id)
        || publication.editions.is_empty()
        || publication.editions.len() > MAX_EDITIONS
    {
        return fail(ErrorKind::Invalid, "publication-contract");
    }
    let mut editions = BTreeSet::new();
    for edition in &publication.editions {
        if !valid_id(&edition.id) || !valid_id(&edition.language) || !editions.insert(&edition.id) {
            return fail(ErrorKind::Invalid, "edition-identity");
        }
        check_contracts(&edition.outputs)?;
        match &edition.content {
            Content::Film { composition } => check_artifact(composition)?,
            Content::Comic { pages, .. } => {
                if pages.is_empty() || pages.len() > MAX_PAGES {
                    return fail(ErrorKind::Budget, "page-count");
                }
                let mut names = BTreeSet::new();
                let mut spreads: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
                for (index, page) in pages.iter().enumerate() {
                    if !valid_id(&page.id)
                        || !names.insert(&page.id)
                        || !unique_ids(&page.panel_order)
                        || !unique_ids(&page.utterance_order)
                        || (page.kind == PageKind::Story && page.panel_order.is_empty())
                    {
                        return fail(ErrorKind::Invalid, "page-identity-order");
                    }
                    check_artifact(&page.source)?;
                    if let Some(spread) = &page.spread_id {
                        if !valid_id(spread) {
                            return fail(ErrorKind::Invalid, "spread-identity");
                        }
                        spreads.entry(spread).or_default().push(index);
                    }
                }
                if spreads
                    .values()
                    .any(|indices| indices.len() != 2 || indices[1] != indices[0] + 1)
                {
                    return fail(ErrorKind::Invalid, "spread-page-pair");
                }
            }
        }
    }
    Ok(())
}
