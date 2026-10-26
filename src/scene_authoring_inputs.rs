//! Exact local inputs for a selected scene authoring invocation.

use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{AssetRef, NativeAlignment, Scene, ScopedBindings};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

/// Limits for small timing metadata, never media payloads or whole inventories.
pub const ALIGNMENT_BYTES_MAX: usize = 4 * 1024 * 1024;
pub const ALIGNMENT_TOTAL_BYTES_MAX: u64 = 16 * 1024 * 1024;
pub const ALIGNMENT_CUES_MAX: usize = 8192;

/// Read alignments by the logical IDs in the selected scoped bindings.
///
/// The injected reader owns asset resolution (for example a captured Tig view)
/// and storage access. It must enforce the supplied byte bound before allocating
/// and hold any byte guard through its read. REEL checks the returned bytes against
/// the declared hash and size; this does not grant production or release approval.
/// No path inventory, cache location, network access or latest-version fallback is
/// supplied by REEL. A changed Tig selection must first update the scoped binding;
/// bytes from a newer selection cannot silently satisfy an older pinned scene.
pub fn read_verified_alignments_from_assets(
    scene: &Scene,
    language_id: &str,
    scopes: &[&ScopedBindings],
    mut read_asset: impl FnMut(&AssetRef, usize) -> Result<Vec<u8>>,
) -> Result<BTreeMap<String, NativeAlignment>> {
    let lane = scene
        .languages
        .get(language_id)
        .context("scene language missing")?;
    if lane.cues.len() > ALIGNMENT_CUES_MAX {
        bail!("alignment cue count exceeds metadata budget");
    }
    let mut requests = Vec::with_capacity(lane.cues.len());
    let mut ids = std::collections::BTreeSet::new();
    let mut total = 0u64;
    // Preflight the entire set before asking a provider to read any bytes.
    for cue in &lane.cues {
        if cue.cue_id.trim().is_empty() || !ids.insert(&cue.cue_id) {
            bail!("alignment cue identities must be nonempty and unique");
        }
        let key = cue
            .phrase_alignment_binding
            .as_deref()
            .context("cue lacks alignment binding")?;
        let asset = selected_alignment(key, scopes)?;
        total = total
            .checked_add(asset.bytes)
            .context("alignment budget overflow")?;
        if asset.bytes == 0
            || asset.bytes > ALIGNMENT_BYTES_MAX as u64
            || total > ALIGNMENT_TOTAL_BYTES_MAX
        {
            bail!("alignment bytes exceed metadata budget");
        }
        requests.push((cue, asset));
    }
    let mut verified = BTreeMap::new();
    for (cue, asset) in requests {
        let bytes = read_asset(asset, asset.bytes as usize)?;
        if bytes.len() as u64 != asset.bytes || hash(&bytes) != asset.sha256 {
            bail!("alignment bytes differ from selected scoped binding");
        }
        let alignment: NativeAlignment = serde_json::from_slice(&bytes)?;
        if alignment.language != language_id || alignment.cue_id != cue.cue_id {
            bail!("alignment language or cue differs from requested identity");
        }
        verified.insert(cue.cue_id.clone(), alignment);
    }
    Ok(verified)
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn selected_alignment<'a>(key: &str, scopes: &[&'a ScopedBindings]) -> Result<&'a AssetRef> {
    let mut matches = scopes.iter().filter_map(|scope| scope.assets.get(key));
    let asset = matches.next().context("alignment binding missing")?;
    if matches.next().is_some()
        || asset.logical_id.trim().is_empty()
        || asset.sha256.len() != 64
        || !asset
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || asset.cache_uri != format!("cache://sha256/{}", asset.sha256)
        || ![
            "selected-private-production",
            "principal-approved",
            "release-cleared",
        ]
        .contains(&asset.selection_state.as_str())
    {
        bail!("alignment binding is ambiguous, invalid or not selected");
    }
    Ok(asset)
}

pub fn read_verified_alignments(
    scene: &Scene,
    language_id: &str,
    manifest_path: &Path,
    scopes: &[&ScopedBindings],
) -> Result<BTreeMap<String, NativeAlignment>> {
    read_verified_alignments_from_bytes(
        scene,
        language_id,
        manifest_path,
        &fs::read(manifest_path)?,
        scopes,
    )
}

/// Compile callers retain and hash this exact manifest buffer rather than
/// re-reading a mutable file after native timing has already been resolved.
pub fn read_verified_alignments_from_bytes(
    scene: &Scene,
    language_id: &str,
    manifest_path: &Path,
    manifest_bytes: &[u8],
    scopes: &[&ScopedBindings],
) -> Result<BTreeMap<String, NativeAlignment>> {
    let paths: BTreeMap<String, String> = serde_json::from_slice(manifest_bytes)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use reel_assembly::scene_authoring::compile_native_event_spans;
    use serde_json::json;

    fn fixture() -> (Scene, ScopedBindings, Vec<u8>) {
        let scene: Scene =
            serde_json::from_str(include_str!("../tests/fixtures/scene-authoring/scene.json"))
                .unwrap();
        let mut bindings: ScopedBindings = serde_json::from_str(include_str!(
            "../tests/fixtures/scene-authoring/scene-bindings.json"
        ))
        .unwrap();
        let bytes = serde_json::to_vec(&json!({
            "schema":"reel.scene-native-alignment.v1", "language":"es", "cue_id":"es-1",
            "selected_take_sha256":"3".repeat(64), "sample_rate":48000,
            "cue_end_sample":48001, "semantic_markers":{"first-line":0}
        }))
        .unwrap();
        let asset = bindings.assets.get_mut("es.alignment").unwrap();
        asset.sha256 = hash(&bytes);
        asset.bytes = bytes.len() as u64;
        asset.cache_uri = format!("cache://sha256/{}", asset.sha256);
        (scene, bindings, bytes)
    }

    #[test]
    fn id_reader_matches_path_reader_and_drives_native_event_clocks() {
        let (scene, bindings, bytes) = fixture();
        let mut requested = Vec::new();
        let by_id =
            read_verified_alignments_from_assets(&scene, "es", &[&bindings], |asset, limit| {
                requested.push(asset.logical_id.clone());
                assert_eq!(limit, bytes.len());
                Ok(bytes.clone())
            })
            .unwrap();
        assert_eq!(requested, ["es-alignment-1"]);
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("timing.json"), &bytes).unwrap();
        let by_path = read_verified_alignments_from_bytes(
            &scene,
            "es",
            &temp.path().join("paths.json"),
            br#"{"es-1":"timing.json"}"#,
            &[&bindings],
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&by_id).unwrap(),
            serde_json::to_value(by_path).unwrap()
        );
        let spans = compile_native_event_spans("es", &scene.languages["es"], &by_id).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_sample, 0);
        assert_eq!(spans[0].end_sample, 48001);
    }

    #[test]
    fn newer_tampered_or_wrong_sized_bytes_cannot_satisfy_an_old_binding() {
        let (scene, bindings, bytes) = fixture();
        let mut changed = bytes.clone();
        changed[0] = b' ';
        let mut long = bytes.clone();
        long.push(b' ');
        for received in [changed, long, Vec::new()] {
            assert!(
                read_verified_alignments_from_assets(&scene, "es", &[&bindings], |_, _| Ok(
                    received.clone()
                ),)
                .is_err()
            );
        }
        assert!(
            read_verified_alignments_from_assets(&scene, "es", &[&bindings], |_, _| bail!(
                "asset unavailable"
            ),)
            .is_err()
        );
    }

    #[test]
    fn whole_request_preflight_refuses_ambiguous_unselected_and_over_budget_bindings() {
        let (scene, bindings, _) = fixture();
        let no_read = |_: &AssetRef, _: usize| -> Result<Vec<u8>> {
            panic!("invalid request must not reach storage")
        };
        assert!(
            read_verified_alignments_from_assets(&scene, "es", &[&bindings, &bindings], no_read)
                .is_err()
        );
        for (field, value) in [
            ("selection_state", json!("draft")),
            ("logical_id", json!(" ")),
            ("sha256", json!("A".repeat(64))),
            ("bytes", json!(0)),
            ("bytes", json!(ALIGNMENT_BYTES_MAX + 1)),
        ] {
            let mut modified = serde_json::to_value(&bindings).unwrap();
            modified["assets"]["es.alignment"][field] = value;
            let modified: ScopedBindings = serde_json::from_value(modified).unwrap();
            assert!(
                read_verified_alignments_from_assets(&scene, "es", &[&modified], no_read).is_err()
            );
        }
        assert!(read_verified_alignments_from_assets(&scene, "es", &[], no_read).is_err());
        assert!(read_verified_alignments_from_assets(&scene, "fr", &[&bindings], no_read).is_err());
        let mut repeated = scene.clone();
        let first = repeated.languages["es"].cues[0].clone();
        repeated.languages.get_mut("es").unwrap().cues.push(first);
        assert!(
            read_verified_alignments_from_assets(&repeated, "es", &[&bindings], no_read).is_err()
        );
        let mut over_total = scene.clone();
        let cue = over_total.languages["es"].cues[0].clone();
        over_total.languages.get_mut("es").unwrap().cues = (0..5)
            .map(|n| {
                let mut cue = cue.clone();
                cue.cue_id = format!("cue-{n}");
                cue
            })
            .collect();
        let mut large = bindings.clone();
        large.assets.get_mut("es.alignment").unwrap().bytes = ALIGNMENT_BYTES_MAX as u64;
        assert!(
            read_verified_alignments_from_assets(&over_total, "es", &[&large], no_read).is_err()
        );
        over_total
            .languages
            .get_mut("es")
            .unwrap()
            .cues
            .resize(ALIGNMENT_CUES_MAX + 1, cue);
        assert!(
            read_verified_alignments_from_assets(&over_total, "es", &[&bindings], no_read).is_err()
        );
    }

    #[test]
    fn hash_valid_cross_language_or_cross_cue_alignment_is_refused() {
        let (scene, bindings, bytes) = fixture();
        for (field, value) in [("language", "en"), ("cue_id", "different-cue")] {
            let mut value_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value_json[field] = json!(value);
            let received = serde_json::to_vec(&value_json).unwrap();
            let mut bindings = bindings.clone();
            let asset = bindings.assets.get_mut("es.alignment").unwrap();
            asset.sha256 = hash(&received);
            asset.bytes = received.len() as u64;
            asset.cache_uri = format!("cache://sha256/{}", asset.sha256);
            assert!(
                read_verified_alignments_from_assets(&scene, "es", &[&bindings], |_, _| Ok(
                    received.clone()
                ),)
                .is_err()
            );
        }
    }
}
