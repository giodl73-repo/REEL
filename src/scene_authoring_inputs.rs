//! Exact local inputs for a selected scene authoring invocation.

use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{NativeAlignment, Scene, ScopedBindings};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

pub fn read_verified_alignments(
    scene: &Scene,
    language_id: &str,
    manifest_path: &Path,
    scopes: &[&ScopedBindings],
) -> Result<BTreeMap<String, NativeAlignment>> {
    let paths: BTreeMap<String, String> = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let lane = scene
        .languages
        .get(language_id)
        .context("scene language missing")?;
    if paths.len() != lane.cues.len() {
        bail!("alignment path set does not match cue set");
    }
    let base = manifest_path
        .parent()
        .context("alignment manifest has no directory")?;
    let mut verified = BTreeMap::new();
    for cue in &lane.cues {
        let relative = paths
            .get(&cue.cue_id)
            .context("cue alignment path missing")?;
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            bail!("alignment paths must be local relative names");
        }
        let bytes = fs::read(base.join(path))?;
        let actual_sha = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let key = cue
            .phrase_alignment_binding
            .as_deref()
            .context("cue lacks alignment binding")?;
        let matches = scopes
            .iter()
            .filter_map(|scope| scope.assets.get(key))
            .collect::<Vec<_>>();
        if matches.len() != 1
            || matches[0].sha256 != actual_sha
            || matches[0].bytes != bytes.len() as u64
            || matches[0].logical_id.is_empty()
            || matches[0].cache_uri != format!("cache://sha256/{actual_sha}")
            || ![
                "selected-private-production",
                "principal-approved",
                "release-cleared",
            ]
            .contains(&matches[0].selection_state.as_str())
        {
            bail!(
                "alignment {} differs from selected scoped bytes",
                cue.cue_id
            );
        }
        verified.insert(cue.cue_id.clone(), serde_json::from_slice(&bytes)?);
    }
    Ok(verified)
}
