use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Component, Path},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::production;

pub const PACKAGE_SCHEMA: &str = "reel.production-package.v0.1";
pub const RECEIPT_SCHEMA: &str = "reel.production-package-receipt.v0.1";
pub const CHECK_SCHEMA: &str = "reel.production-package-check.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionPackage {
    pub schema: String,
    pub work: String,
    pub revision: String,
    pub publication_scope: PublicationScope,
    pub components: Vec<PackageComponent>,
    #[serde(default)]
    pub review_gates: Vec<ReviewGate>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicationScope {
    InternalReview,
    ExternalReview,
    ReleaseCandidate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageComponent {
    pub id: String,
    pub kind: ComponentKind,
    pub path: String,
    pub sha256: String,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentKind {
    ProductionManifest,
    ScorePlan,
    Choreography,
    CraftPlan,
    DepartmentPacket,
    DepartmentReceipt,
    RenderArtifactReport,
    RenderVideo,
    Captions,
    ReviewEvidence,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewGate {
    pub id: String,
    pub owner: String,
    pub status: ReviewGateStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_component: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewGateStatus {
    Pending,
    Approved,
    ChangesRequested,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionPackageReceipt {
    pub schema: String,
    pub package_schema: String,
    pub package_sha256: String,
    pub work: String,
    pub revision: String,
    pub publication_scope: PublicationScope,
    pub components: Vec<ResolvedPackageComponent>,
    pub review_gates: Vec<ReviewGate>,
    pub required_components_verified: bool,
    pub review_gates_approved: bool,
    pub release_ready: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPackageComponent {
    pub id: String,
    pub kind: ComponentKind,
    pub sha256: String,
    pub bytes: u64,
    pub required: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProductionPackageCheckReport {
    pub schema: String,
    pub receipt_sha256: String,
    pub package_sha256: String,
    pub components_verified: usize,
    pub review_gates: usize,
    pub release_ready: bool,
    pub passed: bool,
}

pub fn write_receipt(
    package_path: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<ProductionPackageReceipt> {
    let package_path = package_path.as_ref();
    let package = load(package_path)?;
    let receipt = resolve(package_path, &package)?;
    write_json_new(&receipt, output.as_ref())?;
    Ok(receipt)
}

pub fn check(
    receipt_path: impl AsRef<Path>,
    package_path: impl AsRef<Path>,
) -> Result<ProductionPackageCheckReport> {
    let receipt_path = receipt_path.as_ref();
    let receipt: ProductionPackageReceipt = serde_json::from_slice(&fs::read(receipt_path)?)
        .with_context(|| {
            format!(
                "failed to parse production package receipt {}",
                receipt_path.display()
            )
        })?;
    if receipt.schema != RECEIPT_SCHEMA {
        bail!(
            "unsupported production package receipt schema {}",
            receipt.schema
        );
    }
    let package_path = package_path.as_ref();
    let actual = resolve(package_path, &load(package_path)?)?;
    if serde_json::to_value(&receipt)? != serde_json::to_value(&actual)? {
        bail!(
            "production package receipt does not match package bytes, components, or review gates"
        );
    }
    Ok(ProductionPackageCheckReport {
        schema: CHECK_SCHEMA.to_string(),
        receipt_sha256: production::sha256_path(receipt_path)?,
        package_sha256: actual.package_sha256,
        components_verified: actual.components.len(),
        review_gates: actual.review_gates.len(),
        release_ready: actual.release_ready,
        passed: true,
    })
}

fn load(path: &Path) -> Result<ProductionPackage> {
    let package: ProductionPackage = serde_yaml::from_slice(
        &fs::read(path)
            .with_context(|| format!("failed to read production package {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse production package {}", path.display()))?;
    if package.schema != PACKAGE_SCHEMA {
        bail!("unsupported production package schema {}", package.schema);
    }
    require_text("work", &package.work)?;
    require_text("revision", &package.revision)?;
    if package.components.is_empty() {
        bail!("production package must declare at least one component");
    }
    Ok(package)
}

fn resolve(package_path: &Path, package: &ProductionPackage) -> Result<ProductionPackageReceipt> {
    let root = package_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    let mut ids = BTreeSet::new();
    let mut resolved = Vec::new();
    for component in &package.components {
        validate_id("component", &component.id)?;
        if !ids.insert(component.id.as_str()) {
            bail!("duplicate production package component {}", component.id);
        }
        require_hash(&component.sha256)?;
        let relative = Path::new(&component.path);
        if relative.is_absolute()
            || relative.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            bail!(
                "production package component {} path must stay package-relative",
                component.id
            );
        }
        let path = root.join(relative).canonicalize().with_context(|| {
            format!(
                "failed to resolve production package component {}",
                component.id
            )
        })?;
        if !path.starts_with(&root) {
            bail!(
                "production package component {} escapes package root",
                component.id
            );
        }
        let actual = production::sha256_path(&path)?;
        if actual != component.sha256 {
            bail!(
                "production package component {} hash mismatch: expected {}, found {}",
                component.id,
                component.sha256,
                actual
            );
        }
        resolved.push(ResolvedPackageComponent {
            id: component.id.clone(),
            kind: component.kind,
            sha256: actual,
            bytes: fs::metadata(path)?.len(),
            required: component.required,
        });
    }
    let mut gate_ids = BTreeSet::new();
    for gate in &package.review_gates {
        validate_id("review gate", &gate.id)?;
        require_text(&format!("review gate {} owner", gate.id), &gate.owner)?;
        if !gate_ids.insert(gate.id.as_str()) {
            bail!("duplicate review gate {}", gate.id);
        }
        if gate.status == ReviewGateStatus::Approved {
            let evidence = gate.evidence_component.as_deref().ok_or_else(|| {
                anyhow!(
                    "approved review gate {} requires evidence_component",
                    gate.id
                )
            })?;
            if !ids.contains(evidence) {
                bail!(
                    "review gate {} references unknown evidence component {}",
                    gate.id,
                    evidence
                );
            }
        } else if gate
            .evidence_component
            .as_deref()
            .is_some_and(|id| !ids.contains(id))
        {
            bail!(
                "review gate {} references unknown evidence component",
                gate.id
            );
        }
    }
    let required_components_verified = resolved.iter().filter(|item| item.required).count()
        == package
            .components
            .iter()
            .filter(|item| item.required)
            .count();
    let review_gates_approved = !package.review_gates.is_empty()
        && package
            .review_gates
            .iter()
            .all(|gate| gate.status == ReviewGateStatus::Approved);
    Ok(ProductionPackageReceipt {
        schema: RECEIPT_SCHEMA.to_string(),
        package_schema: package.schema.clone(),
        package_sha256: production::sha256_path(package_path)?,
        work: package.work.clone(),
        revision: package.revision.clone(),
        publication_scope: package.publication_scope,
        components: resolved,
        review_gates: package.review_gates.clone(),
        required_components_verified,
        review_gates_approved,
        release_ready: required_components_verified && review_gates_approved,
    })
}

fn write_json_new<T: Serialize>(value: &T, output: &Path) -> Result<()> {
    if output.exists() {
        bail!(
            "refusing to overwrite production package receipt {}",
            output.display()
        );
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(format!("{}\n", serde_json::to_string_pretty(value)?).as_bytes())?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "failed to publish production package receipt {}",
                output.display()
            )
        })?;
    Ok(())
}

fn require_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn require_hash(value: &str) -> Result<()> {
    if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        bail!("component sha256 must be a 64-character hexadecimal hash");
    }
    Ok(())
}

fn validate_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        bail!("{kind} id {id:?} must use ASCII letters, numbers, hyphens, or underscores");
    }
    Ok(())
}
