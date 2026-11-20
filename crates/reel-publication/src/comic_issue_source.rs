//! Portable issue partitions. No renderer, filesystem access or job admission.
use crate::comic_source::{Limits, Object, Pin};
use crate::{
    ArtifactSet, ComicPage, Content, Error, ErrorKind, Publication, Result, fail, valid_id,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assembly {
    pub schema: String,
    pub plan: Pin,
    /// Part ID -> exact packet receipt. Paths are private, request-relative.
    pub parts: BTreeMap<String, Pin>,
    /// Optional immutable editable-packet carrier per part. Receipt remains separate.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bundles: BTreeMap<String, Pin>,
    pub limits: Limits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<Composition>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub intent: Pin,
    /// Part ID -> exact workflow producer task ID; native origins must verify it.
    pub producers: BTreeMap<String, String>,
}
impl Assembly {
    pub fn check_version(&self) -> Result<()> {
        match (self.schema.as_str(), self.composition.is_some()) {
            ("reel.comic-issue-assembly.v1", false) | ("reel.comic-issue-assembly.v2", true) => {
                Ok(())
            }
            _ => fail(ErrorKind::Invalid, "issue-assembly-version-envelope"),
        }
    }
}
pub fn check_composition(
    plan: &Plan,
    spec: &Composition,
    intent: &crate::composition::Intent,
) -> Result<()> {
    intent.check()?;
    plan.check()?;
    let edition = intent.editions.get(&plan.edition_id).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "issue-composition-edition",
    })?;
    let publication = plan
        .publication
        .editions
        .iter()
        .find(|e| e.id == plan.edition_id)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "issue-composition-publication",
        })?;
    if intent.format != crate::composition::Format::ComicIssue
        || edition.language != publication.language
        || spec.producers.keys().collect::<BTreeSet<_>>()
            != plan.parts.iter().map(|p| &p.id).collect()
        || !plan
            .parts
            .iter()
            .map(|p| &spec.producers[&p.id])
            .eq(edition.components.iter())
    {
        return fail(ErrorKind::Conflict, "issue-composition-order");
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyReceipt {
    pub schema: String,
    pub request: Object,
    pub plan: Object,
    pub parts: BTreeMap<String, Object>,
    pub page_order: Vec<String>,
    pub outputs: ArtifactSet,
    pub human_approval: bool,
    pub retained_owner_acknowledged: bool,
    pub print_ready: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    pub pages: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub publication: Publication,
    pub edition_id: String,
    /// Authored contiguous groups, in reading order; never silently chunked.
    pub parts: Vec<Part>,
}
impl Plan {
    pub fn check(&self) -> Result<()> {
        if serde_json::to_vec(self)
            .map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "issue-plan-json",
            })?
            .len()
            > crate::MAX_CONTROL_BYTES
        {
            return fail(ErrorKind::Budget, "issue-plan-bytes");
        }
        crate::check_publication(&self.publication)?;
        if self.schema != "reel.comic-issue-plan.v1" || !(1..=128).contains(&self.parts.len()) {
            return fail(ErrorKind::Invalid, "issue-plan-contract");
        }
        let pages = self.pages()?;
        let mut offset = 0;
        let mut names = BTreeSet::new();
        for part in &self.parts {
            if !valid_id(&part.id)
                || !names.insert(part.id.to_ascii_lowercase())
                || !(1..=32).contains(&part.pages.len())
            {
                return fail(ErrorKind::Invalid, "issue-part-identity-count");
            }
            let end = offset + part.pages.len();
            if end > pages.len()
                || !part
                    .pages
                    .iter()
                    .eq(pages[offset..end].iter().map(|p| &p.id))
            {
                return fail(ErrorKind::Conflict, "issue-part-page-coverage");
            }
            if end < pages.len()
                && pages[end - 1].spread_id.is_some()
                && pages[end - 1].spread_id == pages[end].spread_id
            {
                return fail(ErrorKind::Conflict, "issue-part-split-spread");
            }
            offset = end;
        }
        if offset != pages.len() {
            return fail(ErrorKind::Missing, "issue-pages-missing");
        }
        Ok(())
    }
    pub(crate) fn pages(&self) -> Result<&[ComicPage]> {
        let edition = self
            .publication
            .editions
            .iter()
            .find(|e| e.id == self.edition_id)
            .ok_or(Error {
                kind: ErrorKind::Missing,
                code: "issue-edition",
            })?;
        let Content::Comic { pages, .. } = &edition.content else {
            return fail(ErrorKind::Invalid, "comic-issue-required");
        };
        Ok(pages)
    }
    /// Derived, unselected metadata; no commit, custody or workflow admission.
    pub fn part_publication(&self, id: &str) -> Result<Publication> {
        self.check()?;
        let part = self.parts.iter().find(|p| p.id == id).ok_or(Error {
            kind: ErrorKind::Missing,
            code: "issue-part",
        })?;
        let mut edition = self
            .publication
            .editions
            .iter()
            .find(|e| e.id == self.edition_id)
            .ok_or(Error {
                kind: ErrorKind::Missing,
                code: "issue-edition",
            })?
            .clone();
        let Content::Comic { pages, .. } = &mut edition.content else {
            return fail(ErrorKind::Invalid, "comic-issue-required");
        };
        pages.retain(|p| part.pages.contains(&p.id));
        let publication = Publication {
            schema: self.publication.schema.clone(),
            id: self.publication.id.clone(),
            editions: vec![edition],
        };
        crate::check_publication(&publication)?;
        Ok(publication)
    }
}
