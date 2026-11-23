//! Preparatory graph oracles. No media, runtime or station is opened.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    entry: String,
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Node {
    Playback {
        id: String,
        selections: Vec<Selection>,
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
struct Selection {
    language: String,
    edition: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    id: String,
    target: String,
    traversals: Option<u16>,
}

impl Node {
    fn id(&self) -> &str {
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

fn validate(graph: &Graph) -> Result<()> {
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

fn fixture() -> Graph {
    serde_json::from_str(
        r#"{
      "entry":"opening",
      "nodes":[
        {"kind":"playback","id":"opening","next":"choose","selections":[
          {"language":"en","edition":"opening-en"},{"language":"es","edition":"opening-es"}]},
        {"kind":"choice","id":"choose","choices":[
          {"id":"repeat","target":"opening","traversals":1},
          {"id":"example","target":"example"},
          {"id":"summary","target":"end"}]},
        {"kind":"playback","id":"example","next":"end","selections":[
          {"language":"en","edition":"example-en"},{"language":"es","edition":"example-es"}]},
        {"kind":"end","id":"end"}
      ]
    }"#,
    )
    .unwrap()
}

#[test]
fn authored_bilingual_graph_has_two_routes_and_one_bounded_repeat() {
    validate(&fixture()).unwrap();
}

#[test]
fn graph_refuses_missing_target_and_unreachable_node() {
    let mut graph = fixture();
    graph.nodes.push(Node::End {
        id: "orphan".into(),
    });
    assert!(
        validate(&graph)
            .unwrap_err()
            .to_string()
            .contains("unreachable node orphan")
    );
    graph.nodes.pop();
    graph.nodes.remove(2);
    assert!(
        validate(&graph)
            .unwrap_err()
            .to_string()
            .contains("missing target example")
    );
}

#[test]
fn graph_refuses_ambiguous_choice_unbounded_cycle_and_missing_language() {
    let mut graph = fixture();
    let Node::Choice { choices, .. } = &mut graph.nodes[1] else {
        panic!()
    };
    choices[1].id = choices[0].id.clone();
    assert!(
        validate(&graph)
            .unwrap_err()
            .to_string()
            .contains("ambiguous choice")
    );
    let mut graph = fixture();
    let Node::Choice { choices, .. } = &mut graph.nodes[1] else {
        panic!()
    };
    choices[0].traversals = None;
    assert!(
        validate(&graph)
            .unwrap_err()
            .to_string()
            .contains("undeclared cycle")
    );
    let mut graph = fixture();
    let Node::Playback { selections, .. } = &mut graph.nodes[0] else {
        panic!()
    };
    selections[1].language = "en".into();
    assert!(
        validate(&graph)
            .unwrap_err()
            .to_string()
            .contains("each language")
    );
}
