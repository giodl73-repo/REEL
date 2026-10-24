//! Explicit, nonrecursive copies from an immutable artifact package to a new working directory.
use crate::{Result, active_existing, artifacts, config, tiering, validate_roots};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const RECEIPT: &str = "artifact-export-receipt.json";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    record: String,
    version_id: String,
    manifest_sha256: String,
    output: PathBuf,
    max_copy_bytes: u64,
    files: Vec<Mapping>,
    read_roots: Option<Vec<PathBuf>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mapping {
    role: String,
    path: String,
}

fn relative(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 512
        || path.contains(['\\', ':'])
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with(['.', ' ']))
        || Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err("Export target must be a portable relative file path".into());
    }
    for part in path.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.eq_ignore_ascii_case(".git")
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err("Export target uses a reserved name".into());
        }
    }
    Ok(())
}
fn plain(path: &Path, directory: bool) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if reparse
        || meta.file_type().is_symlink()
        || if directory {
            !meta.is_dir()
        } else {
            !meta.is_file()
        }
    {
        return Err("Export paths must be regular local files/directories".into());
    }
    Ok(())
}
fn destination(path: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err("Export output must be an explicit absolute local directory".into());
    }
    validate_roots(
        &[path.to_path_buf()],
        true,
        &crate::PathPolicy::ConfiguredLocalRoots,
    )?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid export directory name")?;
    relative(name)?;
    let parent = path
        .parent()
        .ok_or("Export output requires an existing parent")?;
    for ancestor in parent.ancestors() {
        plain(ancestor, true)?;
    }
    let output = parent.canonicalize()?.join(name);
    let normalized = |p: &Path| {
        p.to_string_lossy()
            .replace('\\', "/")
            .trim_start_matches("//?/")
            .trim_end_matches('/')
            .to_lowercase()
    };
    let out = normalized(&output);
    for root in roots {
        let r = normalized(root);
        if out == r || out.starts_with(&(r + "/")) {
            return Err("Working exports must stay outside cache roots".into());
        }
    }
    if output.ancestors().any(|p| {
        p.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case(".git"))
    }) {
        return Err("Working exports cannot write Git internals".into());
    }
    match fs::symlink_metadata(&output) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Ok(_) => return Err("Export exists; choose a new working directory".into()),
        Err(e) => return Err(e.into()),
    };
    Ok(output)
}
fn copy(source: &Path, target: &Path, sha: &str, size: u64) -> Result<()> {
    plain(source, false)?;
    let source = File::open(source)?;
    if source.metadata()?.len() != size {
        return Err("Export source size changed".into());
    }
    let mut source = source.take(size.checked_add(1).ok_or("Export size overflow")?);
    let mut out = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target)?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 65536];
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes = bytes.checked_add(n as u64).ok_or("Export size overflow")?;
        if bytes > size {
            return Err("Export source grew during copy".into());
        }
        digest.update(&buffer[..n]);
        out.write_all(&buffer[..n])?;
    }
    if bytes != size || format!("{:x}", digest.finalize()) != sha {
        return Err("Export copy hash/size mismatch".into());
    }
    out.sync_all()?;
    Ok(())
}
pub fn export(root: &Path, spec: &Path) -> Result<Value> {
    execute(root, spec, false)
}

pub fn plan(root: &Path, spec: &Path) -> Result<Value> {
    execute(root, spec, true)
}
fn execute(root: &Path, spec: &Path, dry_run: bool) -> Result<Value> {
    let raw = tiering::read_bounded(spec, 128 * 1024)?;
    let request: Request = serde_json::from_slice(&raw)?;
    if request.schema != "reel.artifact-export.v1"
        || request.version_id.trim().is_empty()
        || !crate::valid_oid(&request.manifest_sha256)
        || request.files.is_empty()
        || request.files.len() > 512
        || request.max_copy_bytes > 1024 * 1024 * 1024
    {
        return Err("Invalid or oversized artifact export request".into());
    }
    let cfg = config(root)?;
    let mut storage = cfg.clone();
    if let Some(roots) = request.read_roots {
        if roots.is_empty() || roots.len() > 16 {
            return Err("Export requires 1–16 explicit read roots".into());
        }
        validate_roots(&roots, false, &cfg.path_policy)?;
        storage.cache_roots = roots;
    }
    let mut all_roots = cfg
        .cache_roots
        .iter()
        .chain(&cfg.archive_roots)
        .chain(&storage.cache_roots)
        .cloned()
        .collect::<Vec<_>>();
    for cache in cfg.cache_roots.iter().chain(&storage.cache_roots) {
        match cache.canonicalize() {
            Ok(path) => all_roots.push(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    let output = destination(&request.output, &all_roots)?;
    let (lock, assets) = artifacts::inspect(
        root,
        Path::new(&request.record),
        Some(&request.version_id),
        false,
    )?;
    if lock["package_sha256"] != request.manifest_sha256 {
        return Err("Artifact export manifest lock differs".into());
    }
    let mut targets = BTreeSet::from([RECEIPT.to_owned()]);
    let mut total = 0u64;
    let mut selected = vec![];
    let mut unique = BTreeMap::new();
    for mapping in &request.files {
        relative(&mapping.path)?;
        let path = mapping.path.to_lowercase();
        if !targets.insert(path.clone()) {
            return Err("Duplicate or reserved export target".into());
        }
        let a = assets
            .get(&mapping.role)
            .ok_or("Unknown artifact export role")?;
        let sha = a["sha256"].as_str().ok_or("Missing artifact hash")?;
        let bytes = a["bytes"].as_u64().ok_or("Missing artifact bytes")?;
        total = total.checked_add(bytes).ok_or("Export budget overflow")?;
        if total > request.max_copy_bytes {
            return Err("Explicit artifact copy budget exceeded".into());
        }
        unique.insert(sha.to_owned(), bytes);
        selected.push(json!({"role":mapping.role,"path":mapping.path,"sha256":sha,"bytes":bytes}));
    }
    for target in &targets {
        for (n, _) in target.match_indices('/') {
            if targets.contains(&target[..n]) {
                return Err("Export file/directory targets overlap".into());
            }
        }
    }
    let receipt = json!({"schema":"reel.artifact-export-receipt.v1","artifact_id":lock["artifact_id"],"version_id":request.version_id,"manifest_sha256":request.manifest_sha256,"request_sha256":format!("{:x}",Sha256::digest(&raw)),"files":selected,"payload_bytes_copied":total,"source_objects_verified":unique.len(),"automatic_archive_lookup":false,"cache_payloads_written":0,"selection_changed":false,"creative_authority":"not-granted-by-export"});
    let receipt_bytes = serde_json::to_vec(&receipt)?;
    if receipt_bytes.len() > 256 * 1024 {
        return Err("Artifact export receipt exceeds budget".into());
    }
    let mut sources = BTreeMap::new();
    for (sha, size) in unique {
        let p = active_existing(&storage, &sha, size)?
            .ok_or("Named export asset absent from explicit read roots; archive not probed")?;
        sources.insert(sha, p);
    }
    if dry_run {
        return Ok(
            json!({"schema":"reel.artifact-export-plan.v1","artifact_id":lock["artifact_id"],"version_id":request.version_id,"manifest_sha256":request.manifest_sha256,"request_sha256":receipt["request_sha256"],"files":selected,"planned_copy_bytes":total,"source_objects_verified":sources.len(),"payload_bytes_copied":0,"cache_payloads_written":0,"selection_changed":false,"automatic_archive_lookup":false,"copy_authorized":false}),
        );
    }
    fs::create_dir(&output)?;
    for mapping in &selected {
        let target = output.join(mapping["path"].as_str().unwrap());
        let parent = target.parent().ok_or("Missing export parent")?;
        fs::create_dir_all(parent)?;
        for ancestor in parent.ancestors().take_while(|p| p.starts_with(&output)) {
            plain(ancestor, true)?;
        }
        copy(
            &sources[mapping["sha256"].as_str().unwrap()],
            &target,
            mapping["sha256"].as_str().unwrap(),
            mapping["bytes"].as_u64().unwrap(),
        )?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join(RECEIPT))?;
    file.write_all(&receipt_bytes)?;
    file.sync_all()?;
    Ok(
        json!({"schema":"reel.artifact-export-summary.v1","files":selected.len(),"source_objects_verified":sources.len(),"payload_bytes_copied":total,"cache_payloads_written":0,"selection_changed":false,"automatic_archive_lookup":false,"private_receipt":output.join(RECEIPT)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_changed_source_cannot_publish_an_export_copy() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("source");
        fs::write(&source, b"bad").unwrap();
        let target = d.path().join("copy");
        let expected = format!("{:x}", Sha256::digest(b"cel"));
        assert!(copy(&source, &target, &expected, 3).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"bad");
        assert!(!d.path().join(RECEIPT).exists());
        assert!(copy(&source, &d.path().join("short"), &expected, 2).is_err());
        assert!(!d.path().join("short").exists());
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_working_parents_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let alias = d.path().join("alias");
        std::os::unix::fs::symlink(d.path(), &alias).unwrap();
        assert!(destination(&alias.join("out"), &[]).is_err());
        assert!(!d.path().join("out").exists());
    }
}
