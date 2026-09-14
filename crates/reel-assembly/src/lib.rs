//! REEL's portable semantic assembly contract.
//!
//! The crate deliberately owns only deterministic dependency selection and
//! validation.  It does not select creative assets, synthesize a translation,
//! or make an FFmpeg runtime dependency part of a consumer's manifest.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const GRAPH_SCHEMA: &str = "reel.semantic-assembly.v1";
pub const POINTER_SCHEMA: &str = "reel.selected-pointer.v1";
pub const CACHE_PREFIX: &str = "cache://sha256/";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedPointer {
    pub schema: String,
    pub logical_id: String,
    pub selected_lock: ImmutableRef,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImmutableRef {
    pub logical_id: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub logical_id: String,
    pub cache_uri: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub revision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    pub asset: Asset,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub slot_id: String,
    pub beat_id: String,
    pub lane: Lane,
    pub disposition: Disposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_revision_id: Option<String>,
    #[serde(default)]
    pub revisions: Vec<Revision>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Lane {
    Picture,
    Score,
    Sonic,
    Vfx,
    Presentation,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Disposition {
    Selected,
    ExplicitSilence,
    CleanPicture,
    Deprecated,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticEvent {
    pub event_id: String,
    pub scene_id: String,
    pub language: String,
    pub narration: ImmutableRef,
    pub picture: ImmutableRef,
    pub phrase_start_seconds: f64,
    pub phrase_end_seconds: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub slots: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub schema: String,
    pub lock: ImmutableRef,
    pub slots: Vec<Slot>,
    #[serde(default)]
    pub events: Vec<SemanticEvent>,
    pub nodes: Vec<Node>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Closure {
    pub target: String,
    pub node_ids: Vec<String>,
    pub selected_assets: Vec<Asset>,
    pub semantic_events: Vec<SemanticEvent>,
    pub digest_sha256: String,
}

pub fn validate_pointer(pointer: &SelectedPointer) -> Result<()> {
    if pointer.schema != POINTER_SCHEMA {
        bail!("unsupported pointer schema {}", pointer.schema);
    }
    valid_id("pointer", &pointer.logical_id)?;
    valid_ref(&pointer.selected_lock)
}

pub fn validate_graph(graph: &Graph) -> Result<()> {
    if graph.schema != GRAPH_SCHEMA {
        bail!("unsupported assembly schema {}", graph.schema);
    }
    valid_ref(&graph.lock)?;
    let mut slot_ids = BTreeSet::new();
    for slot in &graph.slots {
        if !slot_ids.insert(slot.slot_id.as_str()) {
            bail!("duplicate slot {}", slot.slot_id);
        }
        valid_slot(slot)?;
    }
    let mut event_ids = BTreeSet::new();
    for event in &graph.events {
        if !event_ids.insert(event.event_id.as_str()) {
            bail!("duplicate semantic event {}", event.event_id);
        }
        valid_event(event)?;
    }
    let nodes: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    if nodes.len() != graph.nodes.len() {
        bail!("duplicate node ID");
    }
    for node in &graph.nodes {
        valid_id("node", &node.id)?;
        for input in &node.inputs {
            if !nodes.contains_key(input.as_str()) {
                bail!("node {} names unknown input {input}", node.id);
            }
        }
        for slot in &node.slots {
            if !slot_ids.contains(slot.as_str()) {
                bail!("node {} names unknown slot {slot}", node.id);
            }
        }
        for event in &node.events {
            if !event_ids.contains(event.as_str()) {
                bail!("node {} names unknown event {event}", node.id);
            }
        }
    }
    Ok(())
}

pub fn closure(graph: &Graph, target: &str) -> Result<Closure> {
    validate_graph(graph)?;
    let nodes: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    if !nodes.contains_key(target) {
        bail!("unknown target {target}");
    }
    let slots: BTreeMap<_, _> = graph
        .slots
        .iter()
        .map(|slot| (slot.slot_id.as_str(), slot))
        .collect();
    let events: BTreeMap<_, _> = graph
        .events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect();
    let mut pending = vec![target];
    let mut visited = BTreeSet::new();
    let mut selected = BTreeMap::new();
    let mut included_events = BTreeMap::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let node = nodes[id];
        for input in &node.inputs {
            pending.push(input);
        }
        for slot_id in &node.slots {
            let slot = slots[slot_id.as_str()];
            if slot.disposition == Disposition::Selected {
                let revision = slot
                    .revisions
                    .iter()
                    .find(|item| Some(&item.revision_id) == slot.selected_revision_id.as_ref())
                    .expect("validated selection");
                selected.insert(slot_id.clone(), revision.asset.clone());
            }
        }
        for event_id in &node.events {
            included_events.insert(event_id.clone(), events[event_id.as_str()].clone());
        }
    }
    let node_ids = visited.into_iter().map(str::to_string).collect::<Vec<_>>();
    let selected_assets = selected.into_values().collect::<Vec<_>>();
    let semantic_events = included_events.into_values().collect::<Vec<_>>();
    let material = serde_json::to_vec(&(
        target,
        &graph.lock,
        &node_ids,
        &selected_assets,
        &semantic_events,
    ))?;
    let digest = Sha256::digest(material);
    let digest_sha256 = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(Closure {
        target: target.into(),
        node_ids,
        selected_assets,
        semantic_events,
        digest_sha256,
    })
}

fn valid_slot(slot: &Slot) -> Result<()> {
    valid_id("slot", &slot.slot_id)?;
    if slot.disposition == Disposition::Selected {
        let selected = slot.selected_revision_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("selected slot {} lacks selected revision", slot.slot_id)
        })?;
        if slot.revisions.is_empty()
            || !slot
                .revisions
                .iter()
                .any(|revision| revision.revision_id == selected)
        {
            bail!("selected slot {} has no matching revision", slot.slot_id);
        }
    } else if slot.selected_revision_id.is_some() {
        bail!(
            "non-selected slot {} cannot select a revision",
            slot.slot_id
        );
    }
    let mut ids = BTreeSet::new();
    for revision in &slot.revisions {
        if !ids.insert(revision.revision_id.as_str()) {
            bail!(
                "slot {} repeats revision {}",
                slot.slot_id,
                revision.revision_id
            );
        }
        if let Some(parent) = &revision.supersedes {
            if !ids.contains(parent.as_str()) {
                bail!(
                    "revision {} must supersede an earlier revision",
                    revision.revision_id
                );
            }
        }
        valid_asset(&revision.asset)?;
    }
    Ok(())
}

fn valid_event(event: &SemanticEvent) -> Result<()> {
    valid_id("event", &event.event_id)?;
    valid_ref(&event.narration)?;
    valid_ref(&event.picture)?;
    if event.language.len() != 2
        || event.phrase_start_seconds < 0.0
        || event.phrase_end_seconds <= event.phrase_start_seconds
    {
        bail!(
            "event {} has invalid language or phrase interval",
            event.event_id
        );
    }
    Ok(())
}
fn valid_asset(asset: &Asset) -> Result<()> {
    valid_id("asset", &asset.logical_id)?;
    if asset.cache_uri != format!("{CACHE_PREFIX}{}", asset.sha256) {
        bail!("asset {} is not content-addressed", asset.logical_id);
    }
    valid_hash(&asset.sha256)
}
fn valid_ref(reference: &ImmutableRef) -> Result<()> {
    valid_id("reference", &reference.logical_id)?;
    valid_hash(&reference.sha256)
}
fn valid_hash(hash: &str) -> Result<()> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("expected full SHA-256");
    }
    Ok(())
}
fn valid_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty() || id.chars().any(char::is_whitespace) {
        bail!("{kind} ID is invalid");
    }
    Ok(())
}
