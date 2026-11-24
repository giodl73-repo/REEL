//! Portable handoff from verified cue intake to an authored scene task.
use crate::{
    Artifact, DependencyResult, Error, ErrorKind, Operation, Plan, Result, digest, fail,
    make_packet, manual,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

const INPUT_MAX: u64 = 4 * 1024 * 1024;

#[derive(Deserialize)]
struct Intake {
    schema: String,
    source_commit: String,
    scenes: Vec<Scene>,
}

#[derive(Deserialize)]
struct Scene {
    id: String,
    inputs: Vec<Artifact>,
    cues: Vec<Cue>,
}

#[derive(Deserialize)]
struct Cue {
    language: String,
}

fn read_pinned(path: &Path, maximum: u64, sha256: &str) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    File::open(path)
        .map_err(|_| Error {
            kind: ErrorKind::Missing,
            code: "cue-scene-input-open".into(),
        })?
        .take(maximum + 1)
        .read_to_end(&mut raw)
        .map_err(|_| Error {
            kind: ErrorKind::Integrity,
            code: "cue-scene-input-read".into(),
        })?;
    if raw.len() as u64 > maximum || digest(&raw) != sha256 {
        return fail(ErrorKind::Integrity, "cue-scene-input-pin");
    }
    Ok(raw)
}

pub struct Export<'a> {
    pub intake: &'a Path,
    pub inventory_sha256: &'a str,
    pub scene_id: &'a str,
    pub plan: Plan,
    pub task_id: &'a str,
    pub attempt: &'a str,
    pub dependencies: BTreeMap<String, DependencyResult>,
    pub output: &'a Path,
}

pub fn export(request: Export<'_>) -> Result<String> {
    let Export {
        intake,
        inventory_sha256,
        scene_id,
        mut plan,
        task_id,
        attempt,
        dependencies,
        output,
    } = request;
    let raw = read_pinned(
        &intake.join("inventory.json"),
        crate::MAX_CONTROL_BYTES as u64,
        inventory_sha256,
    )?;
    let evidence: Intake = serde_json::from_slice(&raw).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "cue-scene-inventory-json".into(),
    })?;
    if evidence.schema != "reel.retained-cue-intake.v1" {
        return fail(ErrorKind::Unsupported, "cue-scene-inventory-schema");
    }
    if plan.source_commit_for(task_id)? != evidence.source_commit {
        return fail(ErrorKind::Stale, "cue-scene-source-commit");
    }
    let mut matching = evidence
        .scenes
        .iter()
        .enumerate()
        .filter(|(_, scene)| scene.id == scene_id);
    let Some((index, scene)) = matching.next() else {
        return fail(ErrorKind::Missing, "cue-scene-id");
    };
    if matching.next().is_some() {
        return fail(ErrorKind::Invalid, "duplicate-cue-scene");
    }
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .expect("snapshot checked task identity");
    if task.operation != Operation::TextProposal
        || !task
            .language
            .as_ref()
            .is_some_and(|language| scene.cues.iter().any(|cue| &cue.language == language))
        || (!task.inputs.is_empty() && task.inputs != scene.inputs)
    {
        return fail(ErrorKind::Conflict, "cue-scene-task-contract");
    }
    let expected = [
        format!("scene-{}-source", index + 1),
        "reviewed-cues".into(),
        "performance-plan".into(),
    ];
    if !scene
        .inputs
        .iter()
        .map(|pin| &pin.logical_id)
        .eq(expected.iter())
    {
        return fail(ErrorKind::Integrity, "cue-scene-input-set");
    }
    let mut controls = Vec::new();
    for pin in &scene.inputs {
        if pin.bytes > INPUT_MAX || pin.media_type != "application/json" {
            return fail(ErrorKind::Invalid, "cue-scene-input-limit");
        }
        let filename = format!("{}.json", pin.logical_id);
        let raw = read_pinned(&intake.join(&filename), pin.bytes, &pin.sha256)?;
        if raw.len() as u64 != pin.bytes {
            return fail(ErrorKind::Integrity, "cue-scene-input-length");
        }
        controls.push((pin.logical_id.clone(), filename, raw));
    }
    task.inputs.clone_from(&scene.inputs);
    let packet = make_packet(&plan, task_id, dependencies)?;
    let output = std::path::absolute(output).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "cue-scene-output-path".into(),
    })?;
    manual::export_work(&packet, attempt, &output)?;
    let mut bindings = BTreeMap::<String, PathBuf>::new();
    for (id, name, raw) in controls {
        let path = output.join(name);
        manual::write_new(&path, &raw)?;
        bindings.insert(id, path);
    }
    let raw = serde_json::to_vec_pretty(&bindings).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "cue-scene-bindings-json".into(),
    })?;
    manual::write_new(&output.join("input-bindings.json"), &raw)?;
    Ok(packet.task_key)
}
