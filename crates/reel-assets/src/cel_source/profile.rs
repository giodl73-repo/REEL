//! Bounded edits of existing native profile gains. No media, resampling or mix.
use super::{editor, project::Project};
use crate::{Result, tiering};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

// Reject duplicate keys recursively before Value normalization can hide them.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Unique {
    Object(#[serde(deserialize_with = "super::project::unique_map")] BTreeMap<String, Unique>),
    Array(Vec<Unique>),
    Null(()),
    Bool(bool),
    Number(serde_json::Number),
    String(String),
}
fn unique(bytes: &[u8]) -> Result<Value> {
    Ok(serde_json::to_value(serde_json::from_slice::<Unique>(
        bytes,
    )?)?)
}
fn gain(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|v| v.is_finite() && v.abs() <= 60.0)
}
/// Expose only the two editable native scalars; absent Sonic stays inherited.
pub fn context(docs: &BTreeMap<String, Value>) -> Option<Value> {
    let p = docs.get("profile.json")?;
    if p["schema"] != "reel.scene-delivery-profile.v1"
        || !gain(&p["score_gain_db"])
        || p.get("sonic_gain_db").is_some_and(|v| !gain(v))
    {
        return None;
    }
    let languages = docs.get("scene.json")?["languages"]
        .as_object()?
        .keys()
        .collect::<Vec<_>>();
    if languages.is_empty() || languages.len() > 16 {
        return None;
    }
    Some(
        json!({"schema":"reel.cel-sorter-profile-context.v1", "affected_languages":languages,
        "score_gain_db":p["score_gain_db"],"sonic_gain_db":p.get("sonic_gain_db"),
        "minimum_gain_db":-60,"maximum_gain_db":60}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    scene_id: String,
    expected_inputs: Value,
    #[serde(default)]
    expected_project: Option<String>,
    affected_languages: Vec<String>,
    acknowledge_shared_profile: bool,
    changes: BTreeMap<String, f64>,
    human_approval: bool,
}
/// Apply explicit scene-wide gain intent into a new candidate directory.
pub fn apply(input: &Path, request: &Path, out: &Path) -> Result<()> {
    let raw_request = unique(&tiering::read_bounded(request, 64 * 1024)?)?;
    let r: Request = serde_json::from_value(raw_request.clone())?;
    let project = Project::open(input)?;
    let snapshot = editor::load_snapshot(&project)?;
    let mut docs = snapshot.controls.clone();
    let available = context(&docs).ok_or("Native profile gain controls unavailable")?;
    let all = available["affected_languages"]
        .as_array()
        .ok_or("Missing profile languages")?
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let requested = r
        .affected_languages
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if r.schema != "reel.cel-sorter-profile-edit.v1"
        || r.scene_id != docs["scene.json"]["scene_id"]
        || r.expected_inputs != snapshot.hashes
        || r.expected_project.as_deref() != project.descriptor_sha256()
        || !r.acknowledge_shared_profile
        || r.human_approval
        || requested != all
        || requested.len() != r.affected_languages.len()
    {
        return Err("Stale profile edit or missing exact scene-wide acknowledgment".into());
    }
    if r.changes.is_empty()
        || r.changes.len() > 2
        || r.changes.iter().any(|(key, v)| {
            !matches!(key.as_str(), "score_gain_db" | "sonic_gain_db")
                || !v.is_finite()
                || v.abs() > 60.0
        })
    {
        return Err("Choose native score/Sonic gains between -60 and 60 dB".into());
    }
    let mut profile = unique(&snapshot.raw["profile.json"])?;
    if r.changes
        .iter()
        .all(|(key, value)| profile[key].as_f64() == Some(*value))
    {
        return Err("Profile proposal has no change".into());
    }
    for (key, value) in r.changes {
        profile[key] = json!(value);
    }
    docs.insert("profile.json".into(), profile);
    // No timeline field is editable; original scene/graph/recording/alignment
    // bytes are reused by the existing native snapshot writer.
    project.write_draft_from_snapshot(out, &docs, &raw_request, &snapshot)
}
