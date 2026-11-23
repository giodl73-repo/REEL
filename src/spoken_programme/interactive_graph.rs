//! Authored bilingual interaction graph and structural validation.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub entry: String,
    pub nodes: Vec<Node>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Node {
    Playback {
        id: String,
        selections: Vec<EditionChoice>,
        next: String,
    },
    Choice {
        id: String,
        choices: Vec<Choice>,
    },
    End {
        id: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditionChoice {
    pub language: String,
    pub edition: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub id: String,
    pub target: String,
    pub labels: BTreeMap<String, String>,
    pub traversals: Option<u16>,
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Self::Playback { id, .. } | Self::Choice { id, .. } | Self::End { id } => id,
        }
    }

    fn targets(&self, include_repeats: bool) -> Vec<&str> {
        match self {
            Self::Playback { next, .. } => vec![next],
            Self::Choice { choices, .. } => choices
                .iter()
                .filter(|choice| include_repeats || choice.traversals.is_none())
                .map(|choice| choice.target.as_str())
                .collect(),
            Self::End { .. } => Vec::new(),
        }
    }
}

fn visit<'a>(
    id: &'a str,
    nodes: &BTreeMap<&'a str, &'a Node>,
    active: &mut BTreeSet<&'a str>,
    complete: &mut BTreeSet<&'a str>,
) -> Result<()> {
    if complete.contains(id) {
        return Ok(());
    }
    ensure!(active.insert(id), "undeclared cycle at {id}");
    for target in nodes[id].targets(false) {
        visit(target, nodes, active, complete)?;
    }
    active.remove(id);
    complete.insert(id);
    Ok(())
}

pub fn validate(graph: &Graph) -> Result<()> {
    let mut nodes = BTreeMap::new();
    for node in &graph.nodes {
        let id = node.id();
        ensure!(
            !id.is_empty() && nodes.insert(id, node).is_none(),
            "duplicate or empty node {id}"
        );
    }
    ensure!(
        nodes.contains_key(graph.entry.as_str()),
        "missing entry {}",
        graph.entry
    );
    for node in nodes.values() {
        for target in node.targets(true) {
            ensure!(
                nodes.contains_key(target),
                "{} has missing target {target}",
                node.id()
            );
        }
        match node {
            Node::Playback { id, selections, .. } => {
                let languages: BTreeSet<_> = selections
                    .iter()
                    .map(|item| item.language.as_str())
                    .collect();
                ensure!(
                    selections.len() == 2
                        && languages == BTreeSet::from(["en", "es"])
                        && selections.iter().all(|item| !item.edition.is_empty()),
                    "{id} needs one edition for each language"
                );
            }
            Node::Choice { id, choices } => {
                let mut ids = BTreeSet::new();
                ensure!(
                    choices
                        .iter()
                        .all(|choice| !choice.id.is_empty() && ids.insert(&choice.id)),
                    "ambiguous choice at {id}"
                );
                ensure!(
                    choices.iter().all(|choice| choice.traversals != Some(0)),
                    "zero repeat budget at {id}"
                );
                for choice in choices {
                    ensure!(
                        choice.labels.len() == 2
                            && ["en", "es"].into_iter().all(|language| {
                                choice
                                    .labels
                                    .get(language)
                                    .is_some_and(|label| !label.trim().is_empty())
                            }),
                        "choice {} at {id} needs one label for each language",
                        choice.id
                    );
                }
                ensure!(
                    !node.targets(false).is_empty(),
                    "choice {id} has no exit after repeats"
                );
            }
            Node::End { .. } => {}
        }
    }
    let mut complete = BTreeSet::new();
    visit(&graph.entry, &nodes, &mut BTreeSet::new(), &mut complete)?;
    for id in nodes.keys() {
        ensure!(complete.contains(id), "unreachable node {id}");
    }
    Ok(())
}
