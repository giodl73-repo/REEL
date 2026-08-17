use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    production,
    production_binding::{self, ProductionBinding, ResolvedProductionBinding},
};

pub const CRAFT_PLAN_SCHEMA: &str = "reel.craft-plan.v0.1";
pub const COVERAGE_SCHEMA: &str = "reel.craft-coverage.v0.1";
pub const DEPARTMENT_PACKET_SCHEMA: &str = "reel.department-packet.v0.1";
pub const DEPARTMENT_PACKET_RECEIPT_SCHEMA: &str = "reel.department-packet-receipt.v0.1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Department {
    Directing,
    Cinematography,
    ProductionDesign,
    Costume,
    HairMakeup,
    Performance,
    Editing,
    Sound,
    Score,
    AnimationVfx,
    Accessibility,
    Provenance,
}

impl Department {
    pub const ALL: [Self; 12] = [
        Self::Directing,
        Self::Cinematography,
        Self::ProductionDesign,
        Self::Costume,
        Self::HairMakeup,
        Self::Performance,
        Self::Editing,
        Self::Sound,
        Self::Score,
        Self::AnimationVfx,
        Self::Accessibility,
        Self::Provenance,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Directing => "directing",
            Self::Cinematography => "cinematography",
            Self::ProductionDesign => "production-design",
            Self::Costume => "costume",
            Self::HairMakeup => "hair-makeup",
            Self::Performance => "performance",
            Self::Editing => "editing",
            Self::Sound => "sound",
            Self::Score => "score",
            Self::AnimationVfx => "animation-vfx",
            Self::Accessibility => "accessibility",
            Self::Provenance => "provenance",
        }
    }
}

impl fmt::Display for Department {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Department {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|department| department.as_str() == value)
            .ok_or_else(|| {
                anyhow!(
                    "unknown department {value}; expected one of {}",
                    Self::ALL
                        .iter()
                        .map(|department| department.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CraftPlan {
    pub schema: String,
    pub plan_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production_binding: Option<ProductionBinding>,
    pub periods: Vec<Period>,
    #[serde(default)]
    pub evidence: BTreeMap<String, EvidenceRecord>,
    #[serde(default)]
    pub assets: BTreeMap<String, AssetRecord>,
    #[serde(default)]
    pub continuity: Vec<ContinuityState>,
    #[serde(default)]
    pub editorial: Vec<EditorialDecision>,
    #[serde(default)]
    pub vfx: Vec<VfxRequirement>,
    #[serde(default)]
    pub departments: BTreeMap<Department, DepartmentState>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Period {
    pub id: String,
    pub label: String,
    pub sequence: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    pub kind: String,
    pub reference: String,
    pub description: String,
    pub distribution: DistributionPolicy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRecord {
    pub kind: String,
    pub reference: String,
    pub description: String,
    pub disclosure: String,
    pub distribution: DistributionPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DistributionPolicy {
    InternalOnly,
    ApprovalRequired,
    Shareable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DistributionScope {
    Internal,
    External,
}

impl FromStr for DistributionScope {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "internal" => Ok(Self::Internal),
            "external" => Ok(Self::External),
            _ => bail!("unknown distribution scope {value}; expected internal or external"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentState {
    pub intent: String,
    #[serde(default)]
    pub source_evidence: Vec<String>,
    #[serde(default)]
    pub continuity_refs: Vec<String>,
    pub owner: String,
    pub status: DepartmentStatus,
    #[serde(default)]
    pub assets: Vec<String>,
    pub human_review_gate: HumanReviewGate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DepartmentStatus {
    Planned,
    InProgress,
    ReadyForReview,
    Blocked,
    NotApplicable,
}

impl DepartmentStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::InProgress => "in-progress",
            Self::ReadyForReview => "ready-for-review",
            Self::Blocked => "blocked",
            Self::NotApplicable => "not-applicable",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HumanReviewGate {
    pub required: bool,
    pub status: HumanReviewStatus,
    pub reviewer_role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reference: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HumanReviewStatus {
    Pending,
    Approved,
    ChangesRequested,
    NotRequired,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuityState {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_group: Option<String>,
    pub subject: String,
    pub period: String,
    pub age: String,
    pub wardrobe: Vec<String>,
    pub hair: String,
    #[serde(default)]
    pub hero_props: Vec<String>,
    pub location_zone: String,
    pub time_of_day: String,
    pub lighting_source: String,
    pub screen_direction: ScreenDirection,
    pub reconstruction_disclosure: ReconstructionDisclosure,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScreenDirection {
    LeftToRight,
    RightToLeft,
    Neutral,
    Mixed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReconstructionDisclosure {
    SourceMedia,
    DisclosedReconstruction,
    NotApplicable,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditorialDecision {
    pub id: String,
    pub period: String,
    pub shot_ref: String,
    pub departments: Vec<Department>,
    pub cut_reason: String,
    pub eye_trace: String,
    pub sound_bridge: String,
    pub protected_hold: ProtectedHold,
    pub movement_motivation: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub continuity_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedHold {
    pub required: bool,
    pub duration_ms: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VfxRequirement {
    pub id: String,
    pub period: String,
    pub shot_ref: String,
    pub departments: Vec<Department>,
    pub layers: Vec<String>,
    pub depth: String,
    pub occlusion: Vec<String>,
    pub reflections: String,
    pub particles: String,
    pub interaction_contacts: Vec<String>,
    pub cleanup_requirements: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub continuity_refs: Vec<String>,
    #[serde(default)]
    pub assets: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LoadedCraftPlan {
    pub path: PathBuf,
    pub source_sha256: String,
    pub plan: CraftPlan,
}

#[derive(Clone, Debug, Serialize)]
pub struct CraftValidationReport {
    pub schema: String,
    pub plan_id: String,
    pub source_sha256: String,
    pub periods: usize,
    pub departments_present: usize,
    pub evidence_records: usize,
    pub assets: usize,
    pub continuity_states: usize,
    pub editorial_decisions: usize,
    pub vfx_requirements: usize,
    pub human_review_approved: usize,
    pub human_review_pending: usize,
    pub production_bound: bool,
    pub passed: bool,
    pub scope: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CraftCoverageReport {
    pub schema: String,
    pub plan_id: String,
    pub source_sha256: String,
    pub required_departments: Vec<Department>,
    pub present_departments: Vec<Department>,
    pub missing_departments: Vec<Department>,
    pub by_status: BTreeMap<String, Vec<Department>>,
    pub pending_human_review: Vec<Department>,
    pub blocked_departments: Vec<Department>,
    pub referenced_evidence: usize,
    pub referenced_assets: usize,
    pub referenced_continuity_states: usize,
    pub unreferenced_evidence: Vec<String>,
    pub unreferenced_assets: Vec<String>,
    pub structurally_complete: bool,
    pub production_bound: bool,
    pub artistic_quality_assessed: bool,
    pub scope: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentPacket {
    pub schema: String,
    pub plan_id: String,
    pub title: String,
    pub source_sha256: String,
    pub department: Department,
    pub distribution_scope: DistributionScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production_binding: Option<ResolvedProductionBinding>,
    pub state: DepartmentState,
    pub periods: Vec<Period>,
    pub evidence: BTreeMap<String, EvidenceRecord>,
    pub assets: BTreeMap<String, AssetRecord>,
    pub continuity: Vec<ContinuityState>,
    pub editorial: Vec<EditorialDecision>,
    pub vfx: Vec<VfxRequirement>,
    pub scope: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentPacketReceipt {
    pub schema: String,
    pub packet_schema: String,
    pub packet_sha256: String,
    pub packet_bytes: u64,
    pub source_sha256: String,
    pub department: Department,
    pub distribution_scope: DistributionScope,
}

#[derive(Clone, Debug, Serialize)]
pub struct DepartmentPacketCheckReport {
    pub schema: String,
    pub packet_sha256: String,
    pub receipt_sha256: String,
    pub department: Department,
    pub distribution_scope: DistributionScope,
    pub passed: bool,
}

pub fn load(path: impl AsRef<Path>) -> Result<LoadedCraftPlan> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read craft plan {}", path.display()))?;
    let plan = serde_yaml::from_str::<CraftPlan>(&source)
        .with_context(|| format!("failed to parse craft plan {}", path.display()))?;
    Ok(LoadedCraftPlan {
        path: path.to_path_buf(),
        source_sha256: production::sha256_path(path)?,
        plan,
    })
}

pub fn validate(loaded: &LoadedCraftPlan) -> Result<CraftValidationReport> {
    let plan = &loaded.plan;
    if plan.schema != CRAFT_PLAN_SCHEMA {
        bail!(
            "unsupported craft plan schema {}; expected {CRAFT_PLAN_SCHEMA}",
            plan.schema
        );
    }
    validate_id("plan", &plan.plan_id)?;
    require_text("craft plan title", &plan.title)?;
    validate_periods(&plan.periods)?;
    let period_ids = plan
        .periods
        .iter()
        .map(|period| period.id.as_str())
        .collect::<BTreeSet<_>>();

    for (id, evidence) in &plan.evidence {
        validate_id("evidence", id)?;
        require_text(&format!("evidence {id} kind"), &evidence.kind)?;
        require_text(&format!("evidence {id} reference"), &evidence.reference)?;
        require_text(&format!("evidence {id} description"), &evidence.description)?;
    }
    for (id, asset) in &plan.assets {
        validate_id("asset", id)?;
        require_text(&format!("asset {id} kind"), &asset.kind)?;
        require_text(&format!("asset {id} reference"), &asset.reference)?;
        require_text(&format!("asset {id} description"), &asset.description)?;
        require_text(&format!("asset {id} disclosure"), &asset.disclosure)?;
    }

    let continuity_by_id = validate_continuity(&plan.continuity, &period_ids)?;
    validate_match_groups(&plan.continuity)?;

    for (department, state) in &plan.departments {
        require_text(&format!("{department} intent"), &state.intent)?;
        require_text(&format!("{department} owner"), &state.owner)?;
        validate_unique_refs(
            &format!("{department} source_evidence"),
            &state.source_evidence,
            plan.evidence.keys().map(String::as_str).collect(),
        )?;
        validate_unique_refs(
            &format!("{department} assets"),
            &state.assets,
            plan.assets.keys().map(String::as_str).collect(),
        )?;
        validate_unique_refs(
            &format!("{department} continuity_refs"),
            &state.continuity_refs,
            continuity_by_id.keys().copied().collect(),
        )?;
        validate_review_gate(*department, &state.human_review_gate)?;
    }

    validate_editorial(
        &plan.editorial,
        &period_ids,
        &plan.evidence,
        &continuity_by_id,
    )?;
    validate_vfx(
        &plan.vfx,
        &period_ids,
        &plan.evidence,
        &plan.assets,
        &continuity_by_id,
    )?;
    let resolved_binding = resolve_production_binding(loaded)?;

    let human_review_approved = plan
        .departments
        .values()
        .filter(|state| state.human_review_gate.status == HumanReviewStatus::Approved)
        .count();
    let human_review_pending = plan
        .departments
        .values()
        .filter(|state| {
            state.human_review_gate.required
                && state.human_review_gate.status != HumanReviewStatus::Approved
        })
        .count();
    Ok(CraftValidationReport {
        schema: plan.schema.clone(),
        plan_id: plan.plan_id.clone(),
        source_sha256: loaded.source_sha256.clone(),
        periods: plan.periods.len(),
        departments_present: plan.departments.len(),
        evidence_records: plan.evidence.len(),
        assets: plan.assets.len(),
        continuity_states: plan.continuity.len(),
        editorial_decisions: plan.editorial.len(),
        vfx_requirements: plan.vfx.len(),
        human_review_approved,
        human_review_pending,
        production_bound: resolved_binding.is_some(),
        passed: true,
        scope: "structural contract and cross-reference validation only; no artistic, cultural, historical, or departmental-quality judgment".to_string(),
    })
}

pub fn coverage(loaded: &LoadedCraftPlan) -> Result<CraftCoverageReport> {
    validate(loaded)?;
    let plan = &loaded.plan;
    let production_bound = resolve_production_binding(loaded)?.is_some();
    let present_departments = Department::ALL
        .into_iter()
        .filter(|department| plan.departments.contains_key(department))
        .collect::<Vec<_>>();
    let missing_departments = Department::ALL
        .into_iter()
        .filter(|department| !plan.departments.contains_key(department))
        .collect::<Vec<_>>();
    let mut by_status: BTreeMap<String, Vec<Department>> = BTreeMap::new();
    let mut pending_human_review = Vec::new();
    let mut blocked_departments = Vec::new();
    let mut evidence = BTreeSet::new();
    let mut assets = BTreeSet::new();
    let mut continuity = BTreeSet::new();
    for (department, state) in &plan.departments {
        by_status
            .entry(state.status.as_str().to_string())
            .or_default()
            .push(*department);
        if state.human_review_gate.required
            && state.human_review_gate.status != HumanReviewStatus::Approved
        {
            pending_human_review.push(*department);
        }
        if state.status == DepartmentStatus::Blocked {
            blocked_departments.push(*department);
        }
        evidence.extend(state.source_evidence.iter().cloned());
        assets.extend(state.assets.iter().cloned());
        continuity.extend(state.continuity_refs.iter().cloned());
    }
    for decision in &plan.editorial {
        evidence.extend(decision.evidence_refs.iter().cloned());
        continuity.extend(decision.continuity_refs.iter().cloned());
    }
    for requirement in &plan.vfx {
        evidence.extend(requirement.evidence_refs.iter().cloned());
        assets.extend(requirement.assets.iter().cloned());
        continuity.extend(requirement.continuity_refs.iter().cloned());
    }
    let unreferenced_evidence = plan
        .evidence
        .keys()
        .filter(|id| !evidence.contains(*id))
        .cloned()
        .collect();
    let unreferenced_assets = plan
        .assets
        .keys()
        .filter(|id| !assets.contains(*id))
        .cloned()
        .collect();
    Ok(CraftCoverageReport {
        schema: COVERAGE_SCHEMA.to_string(),
        plan_id: plan.plan_id.clone(),
        source_sha256: loaded.source_sha256.clone(),
        required_departments: Department::ALL.to_vec(),
        present_departments,
        structurally_complete: missing_departments.is_empty(),
        production_bound,
        missing_departments,
        by_status,
        pending_human_review,
        blocked_departments,
        referenced_evidence: evidence.len(),
        referenced_assets: assets.len(),
        referenced_continuity_states: continuity.len(),
        unreferenced_evidence,
        unreferenced_assets,
        artistic_quality_assessed: false,
        scope: "department presence, references, continuity declarations, and human-review state only; completeness is not approval or artistic quality".to_string(),
    })
}

pub fn department_packet(
    loaded: &LoadedCraftPlan,
    department: Department,
) -> Result<DepartmentPacket> {
    department_packet_for_distribution(loaded, department, DistributionScope::Internal, None)
}

pub fn department_packet_for_distribution(
    loaded: &LoadedCraftPlan,
    department: Department,
    distribution_scope: DistributionScope,
    approval_reference: Option<String>,
) -> Result<DepartmentPacket> {
    validate(loaded)?;
    let plan = &loaded.plan;
    let state = plan
        .departments
        .get(&department)
        .ok_or_else(|| anyhow!("craft plan has no {department} department entry"))?
        .clone();
    let production_binding = resolve_production_binding(loaded)?;
    let editorial = plan
        .editorial
        .iter()
        .filter(|decision| decision.departments.contains(&department))
        .cloned()
        .collect::<Vec<_>>();
    let vfx = plan
        .vfx
        .iter()
        .filter(|requirement| requirement.departments.contains(&department))
        .cloned()
        .collect::<Vec<_>>();

    let mut evidence_ids = state
        .source_evidence
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut asset_ids = state.assets.iter().cloned().collect::<BTreeSet<_>>();
    let mut continuity_ids = state
        .continuity_refs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for decision in &editorial {
        evidence_ids.extend(decision.evidence_refs.iter().cloned());
        continuity_ids.extend(decision.continuity_refs.iter().cloned());
    }
    for requirement in &vfx {
        evidence_ids.extend(requirement.evidence_refs.iter().cloned());
        asset_ids.extend(requirement.assets.iter().cloned());
        continuity_ids.extend(requirement.continuity_refs.iter().cloned());
    }
    let evidence = select_map(&plan.evidence, &evidence_ids);
    let assets = select_map(&plan.assets, &asset_ids);
    enforce_distribution(
        distribution_scope,
        approval_reference.as_deref(),
        evidence
            .iter()
            .map(|(id, record)| ("evidence", id.as_str(), record.distribution)),
        assets
            .iter()
            .map(|(id, record)| ("asset", id.as_str(), record.distribution)),
    )?;
    let continuity = plan
        .continuity
        .iter()
        .filter(|state| continuity_ids.contains(&state.id))
        .cloned()
        .collect::<Vec<_>>();
    let period_ids = continuity
        .iter()
        .map(|state| state.period.as_str())
        .chain(editorial.iter().map(|decision| decision.period.as_str()))
        .chain(vfx.iter().map(|requirement| requirement.period.as_str()))
        .collect::<BTreeSet<_>>();
    let periods = plan
        .periods
        .iter()
        .filter(|period| period_ids.contains(period.id.as_str()))
        .cloned()
        .collect();
    Ok(DepartmentPacket {
        schema: DEPARTMENT_PACKET_SCHEMA.to_string(),
        plan_id: plan.plan_id.clone(),
        title: plan.title.clone(),
        source_sha256: loaded.source_sha256.clone(),
        department,
        distribution_scope,
        approval_reference,
        production_binding,
        state,
        periods,
        evidence,
        assets,
        continuity,
        editorial,
        vfx,
        scope: "least-information department handoff selected by explicit department routing; human review remains external".to_string(),
    })
}

fn enforce_distribution<'a, I, J>(
    scope: DistributionScope,
    approval_reference: Option<&str>,
    evidence: I,
    assets: J,
) -> Result<()>
where
    I: Iterator<Item = (&'static str, &'a str, DistributionPolicy)>,
    J: Iterator<Item = (&'static str, &'a str, DistributionPolicy)>,
{
    if scope == DistributionScope::Internal {
        return Ok(());
    }
    for (kind, id, policy) in evidence.chain(assets) {
        match policy {
            DistributionPolicy::InternalOnly => {
                bail!("{kind} {id} is internal-only and cannot enter an external department packet")
            }
            DistributionPolicy::ApprovalRequired
                if approval_reference.is_none_or(|reference| reference.trim().is_empty()) =>
            {
                bail!("{kind} {id} requires --approval-reference for external distribution")
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolve_production_binding(
    loaded: &LoadedCraftPlan,
) -> Result<Option<ResolvedProductionBinding>> {
    let Some(binding) = &loaded.plan.production_binding else {
        return Ok(None);
    };
    let bound = production_binding::resolve(&loaded.path, binding)?;
    for decision in &loaded.plan.editorial {
        let shot = production_binding::require_shot(&bound.resolved, &decision.shot_ref)?;
        if decision.protected_hold.required
            && decision.protected_hold.duration_ms > shot.duration_ms
        {
            bail!(
                "editorial decision {} protected hold {}ms exceeds bound shot {} duration {}ms",
                decision.id,
                decision.protected_hold.duration_ms,
                shot.shot_id,
                shot.duration_ms
            );
        }
    }
    for requirement in &loaded.plan.vfx {
        production_binding::require_shot(&bound.resolved, &requirement.shot_ref)?;
    }
    Ok(Some(bound.resolved))
}

pub fn write_department_packet(packet: &DepartmentPacket, output: impl AsRef<Path>) -> Result<()> {
    let output = output.as_ref();
    if output.exists() {
        bail!(
            "refusing to overwrite department packet {}",
            output.display()
        );
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(format!("{}\n", serde_json::to_string_pretty(packet)?).as_bytes())?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish department packet {}", output.display()))?;
    Ok(())
}

pub fn write_department_packet_receipt(
    packet_path: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<DepartmentPacketReceipt> {
    let packet_path = packet_path.as_ref();
    let packet: DepartmentPacket =
        serde_json::from_slice(&fs::read(packet_path).with_context(|| {
            format!("failed to read department packet {}", packet_path.display())
        })?)
        .with_context(|| {
            format!(
                "failed to parse department packet {}",
                packet_path.display()
            )
        })?;
    if packet.schema != DEPARTMENT_PACKET_SCHEMA {
        bail!("unsupported department packet schema {}", packet.schema);
    }
    let receipt = DepartmentPacketReceipt {
        schema: DEPARTMENT_PACKET_RECEIPT_SCHEMA.to_string(),
        packet_schema: packet.schema,
        packet_sha256: production::sha256_path(packet_path)?,
        packet_bytes: fs::metadata(packet_path)?.len(),
        source_sha256: packet.source_sha256,
        department: packet.department,
        distribution_scope: packet.distribution_scope,
    };
    write_json_exclusive(&receipt, output.as_ref(), "department packet receipt")?;
    Ok(receipt)
}

pub fn check_department_packet(
    receipt_path: impl AsRef<Path>,
    packet_path: impl AsRef<Path>,
) -> Result<DepartmentPacketCheckReport> {
    let receipt_path = receipt_path.as_ref();
    let packet_path = packet_path.as_ref();
    let receipt: DepartmentPacketReceipt = serde_json::from_slice(&fs::read(receipt_path)?)
        .with_context(|| format!("failed to parse receipt {}", receipt_path.display()))?;
    if receipt.schema != DEPARTMENT_PACKET_RECEIPT_SCHEMA {
        bail!(
            "unsupported department packet receipt schema {}",
            receipt.schema
        );
    }
    let packet: DepartmentPacket = serde_json::from_slice(&fs::read(packet_path)?)
        .with_context(|| format!("failed to parse packet {}", packet_path.display()))?;
    let packet_sha256 = production::sha256_path(packet_path)?;
    if receipt.packet_sha256 != packet_sha256 {
        bail!(
            "department packet hash mismatch: receipt has {}, actual is {}",
            receipt.packet_sha256,
            packet_sha256
        );
    }
    let packet_bytes = fs::metadata(packet_path)?.len();
    if receipt.packet_bytes != packet_bytes
        || receipt.packet_schema != packet.schema
        || receipt.source_sha256 != packet.source_sha256
        || receipt.department != packet.department
        || receipt.distribution_scope != packet.distribution_scope
    {
        bail!("department packet receipt metadata does not match packet");
    }
    Ok(DepartmentPacketCheckReport {
        schema: "reel.department-packet-check.v0.1".to_string(),
        packet_sha256,
        receipt_sha256: production::sha256_path(receipt_path)?,
        department: packet.department,
        distribution_scope: packet.distribution_scope,
        passed: true,
    })
}

fn write_json_exclusive<T: Serialize>(value: &T, output: &Path, label: &str) -> Result<()> {
    if output.exists() {
        bail!("refusing to overwrite {label} {}", output.display());
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(format!("{}\n", serde_json::to_string_pretty(value)?).as_bytes())?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {label} {}", output.display()))?;
    Ok(())
}

fn validate_periods(periods: &[Period]) -> Result<()> {
    if periods.is_empty() {
        bail!("craft plan must declare at least one period");
    }
    let mut ids = BTreeSet::new();
    let mut sequences = BTreeSet::new();
    for period in periods {
        validate_id("period", &period.id)?;
        require_text(&format!("period {} label", period.id), &period.label)?;
        if !ids.insert(period.id.as_str()) {
            bail!("duplicate period id {}", period.id);
        }
        if !sequences.insert(period.sequence) {
            bail!("duplicate period sequence {}", period.sequence);
        }
    }
    if periods
        .windows(2)
        .any(|pair| pair[0].sequence >= pair[1].sequence)
    {
        bail!("craft plan periods must be ordered by strictly increasing sequence");
    }
    Ok(())
}

fn validate_continuity<'a>(
    states: &'a [ContinuityState],
    periods: &BTreeSet<&str>,
) -> Result<BTreeMap<&'a str, &'a ContinuityState>> {
    let mut by_id = BTreeMap::new();
    for state in states {
        validate_id("continuity", &state.id)?;
        if by_id.insert(state.id.as_str(), state).is_some() {
            bail!("duplicate continuity id {}", state.id);
        }
        if !periods.contains(state.period.as_str()) {
            bail!(
                "continuity {} references unknown period {}",
                state.id,
                state.period
            );
        }
        for (field, value) in [
            ("subject", &state.subject),
            ("age", &state.age),
            ("hair", &state.hair),
            ("location_zone", &state.location_zone),
            ("time_of_day", &state.time_of_day),
            ("lighting_source", &state.lighting_source),
        ] {
            require_text(&format!("continuity {} {field}", state.id), value)?;
        }
        if state.wardrobe.is_empty() || state.wardrobe.iter().any(|item| item.trim().is_empty()) {
            bail!("continuity {} requires explicit wardrobe state", state.id);
        }
        if state.hero_props.iter().any(|item| item.trim().is_empty()) {
            bail!("continuity {} has an empty hero prop", state.id);
        }
        if let Some(group) = &state.match_group {
            validate_id("continuity match_group", group)?;
        }
    }
    Ok(by_id)
}

fn validate_match_groups(states: &[ContinuityState]) -> Result<()> {
    let mut first_by_group: BTreeMap<&str, &ContinuityState> = BTreeMap::new();
    for state in states {
        let Some(group) = state.match_group.as_deref() else {
            continue;
        };
        if let Some(first) = first_by_group.get(group) {
            if !continuity_matches(first, state) {
                bail!(
                    "continuity match_group {group} disagrees between {} and {}",
                    first.id,
                    state.id
                );
            }
        } else {
            first_by_group.insert(group, state);
        }
    }
    Ok(())
}

fn continuity_matches(left: &ContinuityState, right: &ContinuityState) -> bool {
    left.subject == right.subject
        && left.period == right.period
        && left.age == right.age
        && left.wardrobe == right.wardrobe
        && left.hair == right.hair
        && left.hero_props == right.hero_props
        && left.location_zone == right.location_zone
        && left.time_of_day == right.time_of_day
        && left.lighting_source == right.lighting_source
        && left.screen_direction == right.screen_direction
        && left.reconstruction_disclosure == right.reconstruction_disclosure
}

fn validate_review_gate(department: Department, gate: &HumanReviewGate) -> Result<()> {
    require_text(
        &format!("{department} human review reviewer_role"),
        &gate.reviewer_role,
    )?;
    if gate.required && gate.status == HumanReviewStatus::NotRequired {
        bail!("{department} human review is required but marked not-required");
    }
    if !gate.required && gate.status != HumanReviewStatus::NotRequired {
        bail!("{department} human review is not required but carries an active status");
    }
    if matches!(
        gate.status,
        HumanReviewStatus::Approved | HumanReviewStatus::ChangesRequested
    ) {
        require_optional_text(
            &format!("{department} approved review reviewed_by"),
            gate.reviewed_by.as_deref(),
        )?;
        require_optional_text(
            &format!("{department} approved review review_reference"),
            gate.review_reference.as_deref(),
        )?;
    } else if gate.reviewed_by.is_some() || gate.review_reference.is_some() {
        bail!(
            "{department} review identity/reference requires approved or changes-requested status"
        );
    }
    Ok(())
}

fn validate_editorial(
    decisions: &[EditorialDecision],
    periods: &BTreeSet<&str>,
    evidence: &BTreeMap<String, EvidenceRecord>,
    continuity: &BTreeMap<&str, &ContinuityState>,
) -> Result<()> {
    let mut ids = BTreeSet::new();
    for decision in decisions {
        validate_id("editorial decision", &decision.id)?;
        if !ids.insert(decision.id.as_str()) {
            bail!("duplicate editorial decision {}", decision.id);
        }
        if !periods.contains(decision.period.as_str()) {
            bail!(
                "editorial decision {} references unknown period {}",
                decision.id,
                decision.period
            );
        }
        require_departments(
            &format!("editorial decision {}", decision.id),
            &decision.departments,
        )?;
        for (field, value) in [
            ("shot_ref", &decision.shot_ref),
            ("cut_reason", &decision.cut_reason),
            ("eye_trace", &decision.eye_trace),
            ("sound_bridge", &decision.sound_bridge),
            ("movement_motivation", &decision.movement_motivation),
        ] {
            require_text(
                &format!("editorial decision {} {field}", decision.id),
                value,
            )?;
        }
        if decision.protected_hold.required {
            if decision.protected_hold.duration_ms == 0 {
                bail!(
                    "editorial decision {} protected hold requires duration_ms",
                    decision.id
                );
            }
            require_text(
                &format!("editorial decision {} protected hold reason", decision.id),
                &decision.protected_hold.reason,
            )?;
        } else if decision.protected_hold.duration_ms != 0 {
            bail!(
                "editorial decision {} has protected hold duration but required=false",
                decision.id
            );
        }
        validate_unique_refs(
            &format!("editorial decision {} evidence_refs", decision.id),
            &decision.evidence_refs,
            evidence.keys().map(String::as_str).collect(),
        )?;
        validate_unique_refs(
            &format!("editorial decision {} continuity_refs", decision.id),
            &decision.continuity_refs,
            continuity.keys().copied().collect(),
        )?;
    }
    Ok(())
}

fn validate_vfx(
    requirements: &[VfxRequirement],
    periods: &BTreeSet<&str>,
    evidence: &BTreeMap<String, EvidenceRecord>,
    assets: &BTreeMap<String, AssetRecord>,
    continuity: &BTreeMap<&str, &ContinuityState>,
) -> Result<()> {
    let mut ids = BTreeSet::new();
    for requirement in requirements {
        validate_id("vfx requirement", &requirement.id)?;
        if !ids.insert(requirement.id.as_str()) {
            bail!("duplicate vfx requirement {}", requirement.id);
        }
        if !periods.contains(requirement.period.as_str()) {
            bail!(
                "vfx requirement {} references unknown period {}",
                requirement.id,
                requirement.period
            );
        }
        require_departments(
            &format!("vfx requirement {}", requirement.id),
            &requirement.departments,
        )?;
        for (field, value) in [
            ("shot_ref", &requirement.shot_ref),
            ("depth", &requirement.depth),
            ("reflections", &requirement.reflections),
            ("particles", &requirement.particles),
        ] {
            require_text(
                &format!("vfx requirement {} {field}", requirement.id),
                value,
            )?;
        }
        validate_nonempty_list("layers", &requirement.id, &requirement.layers)?;
        validate_nonempty_list("occlusion", &requirement.id, &requirement.occlusion)?;
        validate_nonempty_list(
            "interaction_contacts",
            &requirement.id,
            &requirement.interaction_contacts,
        )?;
        validate_nonempty_list(
            "cleanup_requirements",
            &requirement.id,
            &requirement.cleanup_requirements,
        )?;
        validate_unique_refs(
            &format!("vfx requirement {} evidence_refs", requirement.id),
            &requirement.evidence_refs,
            evidence.keys().map(String::as_str).collect(),
        )?;
        validate_unique_refs(
            &format!("vfx requirement {} continuity_refs", requirement.id),
            &requirement.continuity_refs,
            continuity.keys().copied().collect(),
        )?;
        validate_unique_refs(
            &format!("vfx requirement {} assets", requirement.id),
            &requirement.assets,
            assets.keys().map(String::as_str).collect(),
        )?;
    }
    Ok(())
}

fn validate_nonempty_list(field: &str, id: &str, values: &[String]) -> Result<()> {
    if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
        bail!("vfx requirement {id} requires explicit {field} entries; use 'none' when applicable");
    }
    Ok(())
}

fn require_departments(context: &str, departments: &[Department]) -> Result<()> {
    if departments.is_empty() {
        bail!("{context} must route to at least one department");
    }
    let unique = departments.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != departments.len() {
        bail!("{context} contains duplicate department routing");
    }
    Ok(())
}

fn validate_unique_refs(context: &str, refs: &[String], allowed: BTreeSet<&str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for reference in refs {
        if !seen.insert(reference.as_str()) {
            bail!("{context} contains duplicate reference {reference}");
        }
        if !allowed.contains(reference.as_str()) {
            bail!("{context} references unknown id {reference}");
        }
    }
    Ok(())
}

fn select_map<T: Clone>(
    source: &BTreeMap<String, T>,
    ids: &BTreeSet<String>,
) -> BTreeMap<String, T> {
    source
        .iter()
        .filter(|(id, _)| ids.contains(*id))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect()
}

fn validate_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("{kind} id {id:?} must use ASCII letters, numbers, hyphens, or underscores");
    }
    Ok(())
}

fn require_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn require_optional_text(field: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => require_text(field, value),
        None => bail!("{field} is required"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture() -> LoadedCraftPlan {
        load("manifests/fixtures/craft-plan/three-period-memoir.yaml").unwrap()
    }

    #[test]
    fn validates_complete_fixture_without_claiming_quality() {
        let loaded = fixture();
        let report = validate(&loaded).unwrap();
        assert_eq!(report.departments_present, 12);
        let coverage = coverage(&loaded).unwrap();
        assert!(coverage.structurally_complete);
        assert!(!coverage.artistic_quality_assessed);
        assert!(coverage.missing_departments.is_empty());
    }

    #[test]
    fn coverage_names_a_missing_department_without_failing_validation() {
        let mut loaded = fixture();
        loaded.plan.departments.remove(&Department::Score);
        validate(&loaded).unwrap();
        let coverage = coverage(&loaded).unwrap();
        assert!(!coverage.structurally_complete);
        assert_eq!(coverage.missing_departments, vec![Department::Score]);
        assert!(!coverage.artistic_quality_assessed);
    }

    #[test]
    fn department_packet_excludes_unrouted_department_information() {
        let loaded = fixture();
        let packet = department_packet(&loaded, Department::Costume).unwrap();
        assert_eq!(packet.department, Department::Costume);
        assert!(packet.evidence.contains_key("ev-photo-early"));
        assert!(!packet.evidence.contains_key("ev-audio-late"));
        assert!(packet.editorial.is_empty());
        assert!(packet.vfx.is_empty());
    }

    #[test]
    fn rejects_disagreement_inside_continuity_match_group() {
        let mut loaded = fixture();
        loaded.plan.continuity[1].hair = "different".to_string();
        let error = validate(&loaded).unwrap_err().to_string();
        assert!(error.contains("match_group early-look disagrees"));
    }

    #[test]
    fn packet_write_refuses_overwrite() {
        let loaded = fixture();
        let packet = department_packet(&loaded, Department::Sound).unwrap();
        let directory = tempdir().unwrap();
        let output = directory.path().join("sound.json");
        write_department_packet(&packet, &output).unwrap();
        assert!(write_department_packet(&packet, &output).is_err());
    }
}
