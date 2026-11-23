//! Authored local demo bindings and terminal commands.
use super::{ChoiceKey, Graph, Selection, Session};
use crate::spoken_programme::checker::{CheckedPackage, TranscriptEntry};
use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Demo {
    graph: Graph,
    bindings: Vec<Binding>,
    ending: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    node: String,
    edition: String,
    language: String,
    module: String,
    programme_sha256: String,
    access_sha256: String,
    first_frame: u64,
    end_frame: u64,
}

impl Demo {
    pub fn start<'a>(&'a self, package: &CheckedPackage, language: &'a str) -> Result<Session<'a>> {
        anyhow::ensure!(
            self.ending.len() == 2
                && ["en", "es"].iter().all(|language| self
                    .ending
                    .get(*language)
                    .is_some_and(|text| !text.trim().is_empty())),
            "demo requires authored English and Spanish endings"
        );
        let bindings: BTreeMap<_, _> = self
            .bindings
            .iter()
            .map(|item| {
                (
                    (item.node.as_str(), item.language.as_str()),
                    Selection {
                        edition: &item.edition,
                        language: &item.language,
                        module: &item.module,
                        programme_sha256: &item.programme_sha256,
                        access_sha256: &item.access_sha256,
                        first_frame: item.first_frame,
                        end_frame: item.end_frame,
                    },
                )
            })
            .collect();
        anyhow::ensure!(
            bindings.len() == self.bindings.len(),
            "duplicate demo binding"
        );
        Session::start(&self.graph, package, bindings, language)
    }

    pub fn ending(&self, language: &str) -> Option<&str> {
        self.ending.get(language).map(String::as_str)
    }
}

/// Return only the verified access entries overlapping the selected module.
pub fn transcript<'a>(
    session: &Session<'_>,
    package: &'a CheckedPackage,
) -> Result<Vec<&'a TranscriptEntry>> {
    let Some(span) = session.selection() else {
        return Ok(Vec::new());
    };
    super::admit_checked_selection(package, span)?;
    let edition = package
        .access()
        .editions
        .iter()
        .find(|edition| edition.edition_id == span.edition && edition.language == span.language)
        .ok_or_else(|| anyhow!("checked transcript unavailable"))?;
    Ok(edition
        .entries
        .iter()
        .filter(|entry| entry.start_sample < span.end_frame && entry.end_sample > span.first_frame)
        .collect())
}

/// Translate a terminal line to the same visit-scoped inputs as keyboard focus.
pub fn command(session: &mut Session<'_>, visit: u64, line: &str) -> Result<bool> {
    let (verb, operand) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
    let current = visit == session.visit();
    let accepted = match (verb, operand) {
        ("next", "") => session.choice_key(visit, ChoiceKey::Next),
        ("previous", "") => session.choice_key(visit, ChoiceKey::Previous),
        ("activate", "") => session.choice_key(visit, ChoiceKey::Activate),
        ("choose", id) => session.choose(visit, id),
        ("pause", "") => session.set_paused(visit, true),
        ("resume", "") => session.set_paused(visit, false),
        ("seek", frame) => session.seek(visit, frame.parse()?),
        ("language", language @ ("en" | "es")) if current => {
            session.restart_in(if language == "en" { "en" } else { "es" })?;
            true
        }
        ("close", "") if current => {
            session.close();
            true
        }
        _ => false,
    };
    Ok(accepted)
}
