//! Exact artifact names remain independent of portable producer file paths.
//! No extraction, file access, materialization or lifecycle authority.
use crate::{error, staging::portable};
use reel_workflow::{Artifact, ArtifactSet, ErrorKind, Result};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::Object;

pub(crate) fn identity(outputs: &ArtifactSet) -> BTreeMap<String, String> {
    outputs.keys().map(|n| (n.clone(), n.clone())).collect()
}

/// Bind outputs and the separate worker receipt to physical file identities.
/// Editable companion files remain in the owned manifest; the capability's
/// independent producer check must verify their complete set and exact bytes.
pub(crate) fn check(
    outputs: &ArtifactSet,
    receipt: &Artifact,
    paths: &BTreeMap<String, String>,
    files: &BTreeMap<String, Object>,
) -> Result<()> {
    if outputs.len() > reel_publication::MAX_OUTPUTS || files.len() > crate::staging::MAX_FILES {
        return Err(error(ErrorKind::Budget, "worker-output-layout-count"));
    }
    if paths.keys().ne(outputs.keys()) || files.is_empty() {
        return Err(error(ErrorKind::Integrity, "worker-output-layout-closure"));
    }
    let mut folded = BTreeSet::from(["receipt.json".to_owned()]);
    for path in paths.values() {
        if !portable(path) {
            return Err(error(ErrorKind::Invalid, "worker-output-layout-path"));
        }
        if !folded.insert(path.to_ascii_lowercase()) {
            return Err(error(ErrorKind::Conflict, "worker-output-layout-alias"));
        }
    }
    let mut folded = BTreeSet::new();
    for path in files.keys() {
        if !portable(path) || !folded.insert(path.to_ascii_lowercase()) {
            return Err(error(ErrorKind::Invalid, "worker-output-layout-file-path"));
        }
    }
    for path in &folded {
        for (at, _) in path.match_indices('/') {
            if folded.contains(&path[..at]) {
                return Err(error(ErrorKind::Conflict, "worker-output-layout-directory"));
            }
        }
    }
    for (path, artifact) in paths
        .iter()
        .map(|(name, path)| (path.as_str(), &outputs[name]))
        .chain(std::iter::once(("receipt.json", receipt)))
    {
        let actual = files
            .get(path)
            .ok_or_else(|| error(ErrorKind::Missing, "worker-output-layout-file"))?;
        if actual.bytes != artifact.bytes || actual.sha256.as_str() != artifact.sha256 {
            return Err(error(ErrorKind::Integrity, "worker-output-layout-pin"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(raw: &[u8]) -> Artifact {
        Artifact {
            logical_id: "test/output".into(),
            sha256: reel_workflow::digest(raw),
            bytes: raw.len() as u64,
            media_type: "application/octet-stream".into(),
        }
    }
    fn layout(
        paths: &[&str],
    ) -> (
        ArtifactSet,
        BTreeMap<String, String>,
        BTreeMap<String, Object>,
    ) {
        let mut outputs = ArtifactSet::new();
        let mut mapping = BTreeMap::new();
        let mut files = BTreeMap::from([("receipt.json".into(), Object::from_bytes(b"receipt"))]);
        for (i, path) in paths.iter().enumerate() {
            let name = format!("artifact-{i}");
            outputs.insert(name.clone(), asset(b"picture"));
            mapping.insert(name, (*path).into());
            files.insert((*path).into(), Object::from_bytes(b"picture"));
        }
        (outputs, mapping, files)
    }
    #[test]
    fn nested_renderer_paths_preserve_artifact_names_and_exact_bytes() {
        let (outputs, paths, files) = layout(&["motion/frames/0001.png", "fonts/review.ttf"]);
        check(&outputs, &asset(b"receipt"), &paths, &files).unwrap();
        let mut changed = files.clone();
        changed.insert(
            "motion/frames/0001.png".into(),
            Object::from_bytes(b"changed"),
        );
        assert_eq!(
            check(&outputs, &asset(b"receipt"), &paths, &changed)
                .unwrap_err()
                .kind,
            ErrorKind::Integrity
        );
        assert!(check(&outputs, &asset(b"wrong receipt"), &paths, &files).is_err());
    }
    #[test]
    fn aliases_traversal_devices_and_receipt_collisions_refuse() {
        for paths in [
            vec!["frames/a.png", "FRAMES/A.PNG"],
            vec!["frames", "frames/a.png"],
            vec!["receipt.json"],
            vec!["receipt.json/extra"],
            vec!["../escape"],
            vec!["/absolute"],
            vec!["frames\\escape"],
            vec!["aux/frame.png"],
        ] {
            let (outputs, mapping, files) = layout(&paths);
            assert!(
                check(&outputs, &asset(b"receipt"), &mapping, &files).is_err(),
                "{paths:?}"
            );
        }
    }
    #[test]
    fn editable_companions_remain_and_missing_or_retargeted_outputs_refuse() {
        let (outputs, paths, mut files) = layout(&["frames/a.png"]);
        files.insert("sources/request.json".into(), Object::from_bytes(b"source"));
        check(&outputs, &asset(b"receipt"), &paths, &files).unwrap();
        files.insert("sources".into(), Object::from_bytes(b"collision"));
        assert!(check(&outputs, &asset(b"receipt"), &paths, &files).is_err());
        files.remove("sources");
        files.remove("frames/a.png");
        files.insert("frames/b.png".into(), Object::from_bytes(b"picture"));
        assert!(check(&outputs, &asset(b"receipt"), &paths, &files).is_err());
        assert!(check(&outputs, &asset(b"receipt"), &BTreeMap::new(), &files).is_err());
    }
    #[test]
    fn legacy_flat_names_keep_the_same_mapping() {
        let (mut outputs, _, _) = layout(&["page.png"]);
        let artifact = outputs.remove("artifact-0").unwrap();
        outputs.insert("page.png".into(), artifact);
        let paths = identity(&outputs);
        assert_eq!(
            paths,
            BTreeMap::from([("page.png".into(), "page.png".into())])
        );
        check(
            &outputs,
            &asset(b"receipt"),
            &paths,
            &BTreeMap::from([
                ("page.png".into(), Object::from_bytes(b"picture")),
                ("receipt.json".into(), Object::from_bytes(b"receipt")),
            ]),
        )
        .unwrap();
    }
}
