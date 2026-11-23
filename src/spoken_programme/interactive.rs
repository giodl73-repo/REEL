//! Admission of exact module spans from verified spoken-programme access.
#[path = "interactive_graph.rs"]
mod graph;

pub use graph::{Choice, EditionChoice, Graph, Node, validate};

use super::checker::CheckedPackage;
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection<'a> {
    pub edition: &'a str,
    pub language: &'a str,
    pub module: &'a str,
    pub programme_sha256: &'a str,
    pub access_sha256: &'a str,
    pub first_frame: u64,
    pub end_frame: u64,
}

pub fn admit_selection<'a>(
    requested: Selection<'a>,
    checked: &[Selection<'a>],
) -> Result<Selection<'a>> {
    let mut candidates = checked.iter().filter(|item| {
        item.edition == requested.edition
            && item.language == requested.language
            && item.module == requested.module
    });
    let observed = candidates
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing checked edition/language/module selection"))?;
    ensure!(candidates.next().is_none(), "ambiguous checked selection");
    ensure!(
        requested.programme_sha256 == observed.programme_sha256,
        "checked programme identity differs"
    );
    ensure!(
        requested.access_sha256 == observed.access_sha256,
        "checked access identity differs"
    );
    ensure!(
        requested.first_frame < requested.end_frame
            && requested.first_frame == observed.first_frame
            && requested.end_frame == observed.end_frame,
        "checked media span differs"
    );
    Ok(*observed)
}

/// Bind an authored selection to the verified audio and transcript for its module.
pub fn admit_checked_selection(package: &CheckedPackage, requested: Selection<'_>) -> Result<()> {
    ensure!(
        package.summary().access_checked,
        "programme access is unchecked"
    );
    let candidates = package
        .access()
        .editions
        .iter()
        .filter(|edition| {
            edition.edition_id == requested.edition && edition.language == requested.language
        })
        .flat_map(|edition| {
            edition.chapters.iter().map(|chapter| Selection {
                edition: &edition.edition_id,
                language: &edition.language,
                module: &chapter.module_id,
                programme_sha256: &edition.wave.sha256,
                access_sha256: &edition.transcript.sha256,
                first_frame: chapter.start_sample,
                end_frame: chapter.end_sample,
            })
        })
        .collect::<Vec<_>>();
    admit_selection(requested, &candidates)?;
    Ok(())
}
