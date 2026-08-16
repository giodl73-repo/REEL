use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::Builder;

use crate::{
    adapters::still_animatic::{AnimaticRenderReport, check_animatic},
    production::{self, LoadedProductionManifest, TimingStatus, VariantLineage},
};

pub const SELECTION_LOCK_SCHEMA: &str = "reel.selection-lock.v0.1";
pub const PLANNING_DERIVATIVE_SCHEMA: &str = "reel.planning-derivative.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionLockReceipt {
    pub schema: String,
    pub tool_version: String,
    pub created_unix: u64,
    pub work: String,
    pub source_manifest_sha256: String,
    pub locked_manifest: String,
    pub locked_manifest_sha256: String,
    pub selected_artifact: String,
    pub selected_artifact_sha256: String,
    pub selected_output_sha256: String,
    pub selected_output_bytes: u64,
    pub selected_output_duration_ms: u64,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SelectionLockReport {
    pub packet: String,
    pub receipt: String,
    pub locked_manifest: String,
    pub selected_artifact: String,
    pub work: String,
    pub selected_output_sha256: String,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SelectionLockCheckReport {
    pub packet: String,
    pub work: String,
    pub source_manifest_sha256: String,
    pub locked_manifest_sha256: String,
    pub selected_artifact_sha256: String,
    pub selected_output_sha256: String,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlanningDerivativeReport {
    pub schema: String,
    pub source_manifest: String,
    pub source_manifest_sha256: String,
    pub output_manifest: String,
    pub output_manifest_sha256: String,
    pub work: String,
    pub timing_status: String,
    pub transformation_reason: String,
    pub changed_dimensions: Vec<String>,
}

pub fn lock_selection(
    artifact_manifest: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> Result<SelectionLockReport> {
    let artifact_manifest = artifact_manifest.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve artifact report {}",
            artifact_manifest.as_ref().display()
        )
    })?;
    check_animatic(&artifact_manifest)?;
    let artifact_bytes = fs::read(&artifact_manifest)?;
    let artifact: AnimaticRenderReport =
        serde_json::from_slice(&artifact_bytes).context("artifact report is not valid JSON")?;
    let source = artifact
        .inputs
        .iter()
        .find(|input| input.kind == "manifest")
        .ok_or_else(|| anyhow!("artifact report has no manifest input"))?;
    let source_manifest = PathBuf::from(&source.path)
        .canonicalize()
        .with_context(|| format!("failed to resolve source manifest {}", source.path))?;
    if production::sha256_path(&source_manifest)? != source.sha256 {
        bail!("source manifest SHA-256 no longer matches selected artifact");
    }
    let loaded = production::load(&source_manifest)?;
    production::validate(&loaded)?;
    let mut locked = loaded.manifest.clone();
    locked.timing_status = TimingStatus::Locked;
    locked.lineage = Some(VariantLineage {
        parent_manifest: source_manifest.display().to_string(),
        root_work: loaded
            .manifest
            .lineage
            .as_ref()
            .map(|lineage| lineage.root_work.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| loaded.manifest.work.clone()),
        scene_key: loaded
            .manifest
            .scenes
            .iter()
            .map(|scene| scene.id.as_str())
            .collect::<Vec<_>>()
            .join("+"),
        transformation_reason: "selected proof lock".to_string(),
        changed_dimensions: vec!["governance".to_string()],
        review_candidate: false,
        principal_approved: false,
        created_unix: unix_now()?,
    });
    production::validate(&LoadedProductionManifest {
        path: output_dir.as_ref().join("manifest.locked.yaml"),
        manifest: locked.clone(),
        bytes: Vec::new(),
    })?;

    let output_sha256 = artifact
        .output_sha256
        .clone()
        .ok_or_else(|| anyhow!("selected artifact has no output SHA-256"))?;
    let output_bytes = artifact
        .output_bytes
        .ok_or_else(|| anyhow!("selected artifact has no output byte length"))?;
    let output_duration_ms = artifact
        .output_duration_ms
        .ok_or_else(|| anyhow!("selected artifact has no measured duration"))?;
    let locked_bytes = serde_yaml::to_string(&locked)?.into_bytes();
    let packet = output_dir.as_ref();
    if packet.exists() {
        bail!("refusing to overwrite lock packet: {}", packet.display());
    }
    let parent = packet.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = Builder::new().prefix(".reel-lock-").tempdir_in(parent)?;
    let locked_name = "manifest.locked.yaml";
    let artifact_name = "selected-artifact.json";
    let receipt_name = "selection-lock.json";
    fs::write(staging.path().join(locked_name), &locked_bytes)?;
    fs::write(staging.path().join(artifact_name), &artifact_bytes)?;
    let receipt = SelectionLockReceipt {
        schema: SELECTION_LOCK_SCHEMA.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        created_unix: unix_now()?,
        work: artifact.work.clone(),
        source_manifest_sha256: source.sha256.clone(),
        locked_manifest: locked_name.to_string(),
        locked_manifest_sha256: production::sha256_path(staging.path().join(locked_name))?,
        selected_artifact: artifact_name.to_string(),
        selected_artifact_sha256: production::sha256_path(staging.path().join(artifact_name))?,
        selected_output_sha256: output_sha256.clone(),
        selected_output_bytes: output_bytes,
        selected_output_duration_ms: output_duration_ms,
        verified: true,
    };
    fs::write(
        staging.path().join(receipt_name),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    let staging_path = staging.keep();
    fs::rename(&staging_path, packet).with_context(|| {
        format!(
            "failed to atomically publish lock packet {}",
            packet.display()
        )
    })?;
    Ok(SelectionLockReport {
        packet: packet.display().to_string(),
        receipt: packet.join(receipt_name).display().to_string(),
        locked_manifest: packet.join(locked_name).display().to_string(),
        selected_artifact: packet.join(artifact_name).display().to_string(),
        work: artifact.work,
        selected_output_sha256: output_sha256,
        verified: true,
    })
}

pub fn check_selection_lock(packet: impl AsRef<Path>) -> Result<SelectionLockCheckReport> {
    let packet = packet.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve lock packet {}",
            packet.as_ref().display()
        )
    })?;
    let receipt: SelectionLockReceipt =
        serde_json::from_slice(&fs::read(packet.join("selection-lock.json"))?)
            .context("selection lock receipt is not valid JSON")?;
    if receipt.schema != SELECTION_LOCK_SCHEMA || !receipt.verified {
        bail!("selection lock receipt is unsupported or unverified");
    }
    let locked_path = confined_join(&packet, &receipt.locked_manifest)?;
    let artifact_path = confined_join(&packet, &receipt.selected_artifact)?;
    if production::sha256_path(&locked_path)? != receipt.locked_manifest_sha256 {
        bail!("locked manifest SHA-256 does not match selection receipt");
    }
    if production::sha256_path(&artifact_path)? != receipt.selected_artifact_sha256 {
        bail!("selected artifact SHA-256 does not match selection receipt");
    }
    let locked = production::load(&locked_path)?;
    production::validate(&locked)?;
    if locked.manifest.timing_status != TimingStatus::Locked {
        bail!("selection packet manifest is not locked");
    }
    let artifact: AnimaticRenderReport = serde_json::from_slice(&fs::read(&artifact_path)?)?;
    let source = artifact
        .inputs
        .iter()
        .find(|input| input.kind == "manifest")
        .ok_or_else(|| anyhow!("selected artifact has no manifest input"))?;
    if source.sha256 != receipt.source_manifest_sha256
        || artifact.output_sha256.as_deref() != Some(&receipt.selected_output_sha256)
        || artifact.output_bytes != Some(receipt.selected_output_bytes)
        || artifact.output_duration_ms != Some(receipt.selected_output_duration_ms)
        || artifact.work != receipt.work
        || locked.manifest.work != receipt.work
    {
        bail!("selection lock receipt lineage does not match its manifest and artifact");
    }
    let source_manifest = PathBuf::from(&source.path).canonicalize()?;
    let source_loaded = production::load(&source_manifest)?;
    let mut normalized_locked = locked.manifest.clone();
    normalized_locked.timing_status = source_loaded.manifest.timing_status;
    normalized_locked.lineage = source_loaded.manifest.lineage.clone();
    if serde_json::to_value(&normalized_locked)? != serde_json::to_value(&source_loaded.manifest)? {
        bail!("locked manifest changes production content beyond lock governance fields");
    }
    check_animatic(&artifact_path)?;
    Ok(SelectionLockCheckReport {
        packet: packet.display().to_string(),
        work: receipt.work,
        source_manifest_sha256: receipt.source_manifest_sha256,
        locked_manifest_sha256: receipt.locked_manifest_sha256,
        selected_artifact_sha256: receipt.selected_artifact_sha256,
        selected_output_sha256: receipt.selected_output_sha256,
        verified: true,
    })
}

pub fn derive_planning_manifest(
    locked_manifest: impl AsRef<Path>,
    output_manifest: impl AsRef<Path>,
    reason: &str,
    changed_dimensions: &[String],
) -> Result<PlanningDerivativeReport> {
    if reason.trim().is_empty() {
        bail!("planning derivative requires a non-empty reason");
    }
    if changed_dimensions.is_empty() || changed_dimensions.iter().any(|item| item.trim().is_empty())
    {
        bail!("planning derivative requires non-empty changed dimensions");
    }
    let source = locked_manifest.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve locked manifest {}",
            locked_manifest.as_ref().display()
        )
    })?;
    let loaded = production::load(&source)?;
    production::validate(&loaded)?;
    if loaded.manifest.timing_status != TimingStatus::Locked {
        bail!("planning derivatives must start from a locked manifest");
    }
    let output = output_manifest.as_ref();
    if output.exists() {
        bail!(
            "refusing to overwrite planning derivative {}",
            output.display()
        );
    }
    let mut derivative = loaded.manifest.clone();
    derivative.timing_status = TimingStatus::Conformed;
    derivative.lineage = Some(VariantLineage {
        parent_manifest: source.display().to_string(),
        root_work: loaded
            .manifest
            .lineage
            .as_ref()
            .map(|lineage| lineage.root_work.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| loaded.manifest.work.clone()),
        scene_key: loaded
            .manifest
            .scenes
            .iter()
            .map(|scene| scene.id.as_str())
            .collect::<Vec<_>>()
            .join("+"),
        transformation_reason: reason.trim().to_string(),
        changed_dimensions: changed_dimensions.to_vec(),
        review_candidate: true,
        principal_approved: false,
        created_unix: unix_now()?,
    });
    production::validate(&LoadedProductionManifest {
        path: output.to_path_buf(),
        manifest: derivative.clone(),
        bytes: Vec::new(),
    })?;
    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_yaml::to_string(&derivative)?.into_bytes();
    let mut temp = Builder::new()
        .prefix(".reel-derivative-")
        .tempfile_in(output.parent().unwrap_or_else(|| Path::new(".")))?;
    use std::io::Write;
    temp.write_all(&bytes)?;
    temp.flush()?;
    temp.persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(PlanningDerivativeReport {
        schema: PLANNING_DERIVATIVE_SCHEMA.to_string(),
        source_manifest: source.display().to_string(),
        source_manifest_sha256: production::sha256_path(&source)?,
        output_manifest: output.display().to_string(),
        output_manifest_sha256: production::sha256_path(output)?,
        work: derivative.work,
        timing_status: derivative.timing_status.as_str().to_string(),
        transformation_reason: reason.trim().to_string(),
        changed_dimensions: changed_dimensions.to_vec(),
    })
}

fn confined_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = root.join(relative).canonicalize()?;
    if !candidate.starts_with(root) {
        bail!("selection receipt path escapes its packet");
    }
    Ok(candidate)
}

fn unix_now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn planning_derivative_unlocks_without_mutating_selected_manifest() {
        let temp = tempdir().unwrap();
        let mut manifest = production::load("manifests/fixtures/vertical-sound-off/manifest.yaml")
            .unwrap()
            .manifest;
        manifest.timing_status = TimingStatus::Locked;
        let locked = temp.path().join("locked.yaml");
        fs::write(&locked, serde_yaml::to_string(&manifest).unwrap()).unwrap();
        let original_hash = production::sha256_path(&locked).unwrap();
        let derivative = temp.path().join("next.yaml");

        let report = derive_planning_manifest(
            &locked,
            &derivative,
            "revise the score",
            &["mix".to_string()],
        )
        .unwrap();

        assert_eq!(production::sha256_path(&locked).unwrap(), original_hash);
        assert_eq!(report.timing_status, "conformed");
        let next = production::load(&derivative).unwrap();
        assert_eq!(next.manifest.timing_status, TimingStatus::Conformed);
        let lineage = next.manifest.lineage.unwrap();
        assert_eq!(lineage.transformation_reason, "revise the score");
        assert_eq!(lineage.changed_dimensions, vec!["mix"]);
        assert!(!lineage.principal_approved);
    }

    #[test]
    fn planning_derivative_rejects_unlocked_sources_and_overwrite() {
        let temp = tempdir().unwrap();
        let source = Path::new("manifests/fixtures/vertical-sound-off/manifest.yaml");
        let output = temp.path().join("next.yaml");
        let error =
            derive_planning_manifest(source, &output, "change", &["edit".to_string()]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("must start from a locked manifest")
        );
    }
}
