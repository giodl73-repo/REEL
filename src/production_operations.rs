use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::production::{self, MediaKind};

pub const GENERATION_INPUT_SCHEMA: &str = "reel.generation-plan-input.v0.1";
pub const GENERATION_PLAN_SCHEMA: &str = "reel.generation-plan.v0.1";
pub const MATERIALIZATION_INPUT_SCHEMA: &str = "reel.materialization-result-input.v0.1";
pub const MATERIALIZATION_RECEIPT_SCHEMA: &str = "reel.materialization-result.v0.1";
pub const ASSET_PROMOTION_INPUT_SCHEMA: &str = "reel.asset-promotion-input.v0.1";
pub const ASSET_PROMOTION_RECORD_SCHEMA: &str = "reel.asset-promotion-record.v0.1";
pub const PICTURE_PLAN_INPUT_SCHEMA: &str = "reel.picture-plan-input.v0.1";
pub const PICTURE_CACHE_INDEX_SCHEMA: &str = "reel.picture-cache-index.v0.1";
pub const PICTURE_PLAN_SCHEMA: &str = "reel.picture-plan.v0.1";
pub const REVIEW_FINDINGS_SCHEMA: &str = "reel.timecoded-review-findings.v0.1";
pub const REPAIR_QUEUE_SCHEMA: &str = "reel.repair-queue.v0.1";
pub const PRODUCTION_STATE_INDEX_SCHEMA: &str = "reel.production-state-index.v0.1";
pub const PRODUCTION_STATE_AUDIT_SCHEMA: &str = "reel.production-state-audit.v0.1";
pub const VOICE_TAKE_INPUT_SCHEMA: &str = "reel.voice-take-ledger-input.v0.1";
pub const VOICE_TAKE_REPORT_SCHEMA: &str = "reel.voice-take-ledger.v0.1";
pub const MUSIC_PROVENANCE_INPUT_SCHEMA: &str = "reel.music-provenance-input.v0.1";
pub const MUSIC_PROVENANCE_REPORT_SCHEMA: &str = "reel.music-provenance.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NamedHash {
    pub id: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalFileHash {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationPlanInput {
    pub schema: String,
    pub production_manifest_sha256: String,
    pub tool_version: String,
    pub units: Vec<GenerationUnitInput>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationUnitInput {
    pub unit_id: String,
    pub shot_id: String,
    pub prompt_sha256: String,
    #[serde(default)]
    pub input_hashes: Vec<NamedHash>,
    pub expected_output: ExpectedVisualOutput,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedVisualOutput {
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationPlan {
    pub schema: String,
    pub source_contract_sha256: String,
    pub production_manifest_sha256: String,
    pub tool_version: String,
    pub units: Vec<GenerationUnit>,
    pub provider_execution_requested: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationUnit {
    pub unit_id: String,
    pub shot_id: String,
    pub prompt_sha256: String,
    pub input_hashes: Vec<NamedHash>,
    pub expected_output: ExpectedVisualOutput,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializationResultInput {
    pub schema: String,
    pub generation_plan_sha256: String,
    pub production_manifest_sha256: String,
    pub outputs: Vec<MaterializedUnitInput>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializedUnitInput {
    pub unit_id: String,
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializationReceipt {
    pub schema: String,
    pub generation_plan_sha256: String,
    pub production_manifest_sha256: String,
    pub outputs: Vec<MaterializedUnit>,
    pub all_outputs_verified: bool,
    pub provider_executed_by_reel: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializedUnit {
    pub unit_id: String,
    pub shot_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub media_type: String,
}

pub fn write_generation_plan(
    manifest_path: impl AsRef<Path>,
    input_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<GenerationPlan> {
    let manifest_path = manifest_path.as_ref();
    let loaded = production::load(manifest_path)?;
    production::validate(&loaded)?;
    let input_path = input_path.as_ref();
    let (input, input_sha256): (GenerationPlanInput, String) = read_contract_with_hash(input_path)?;
    require_schema(&input.schema, GENERATION_INPUT_SCHEMA)?;
    let manifest_sha256 = hash_bytes(&loaded.bytes);
    if input.production_manifest_sha256 != manifest_sha256 {
        bail!("generation plan production manifest hash is stale");
    }
    require_text("tool_version", &input.tool_version)?;
    if input.units.is_empty() {
        bail!("generation plan requires at least one unit");
    }
    let shot_ids = loaded
        .manifest
        .shots
        .iter()
        .map(|shot| shot.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut unit_ids = BTreeSet::new();
    let mut bound_shots = BTreeSet::new();
    let mut units = Vec::new();
    for unit in input.units {
        require_token("unit_id", &unit.unit_id)?;
        require_token("shot_id", &unit.shot_id)?;
        if !unit_ids.insert(unit.unit_id.clone()) {
            bail!("duplicate generation unit {}", unit.unit_id);
        }
        if !bound_shots.insert(unit.shot_id.clone()) {
            bail!(
                "generation plan contains multiple units for shot {}",
                unit.shot_id
            );
        }
        if !shot_ids.contains(unit.shot_id.as_str()) {
            bail!(
                "generation unit {} references unknown shot {}",
                unit.unit_id,
                unit.shot_id
            );
        }
        require_hash(&unit.prompt_sha256)?;
        validate_named_hashes(&unit.input_hashes)?;
        validate_visual_output(&unit.expected_output)?;
        units.push(GenerationUnit {
            unit_id: unit.unit_id,
            shot_id: unit.shot_id,
            prompt_sha256: unit.prompt_sha256,
            input_hashes: unit.input_hashes,
            expected_output: unit.expected_output,
        });
    }
    units.sort_by(|left, right| {
        (&left.shot_id, &left.unit_id).cmp(&(&right.shot_id, &right.unit_id))
    });
    let plan = GenerationPlan {
        schema: GENERATION_PLAN_SCHEMA.to_string(),
        source_contract_sha256: input_sha256,
        production_manifest_sha256: manifest_sha256,
        tool_version: input.tool_version,
        units,
        provider_execution_requested: false,
    };
    write_json_new(&plan, output_path.as_ref())?;
    Ok(plan)
}

pub fn write_materialization_receipt(
    plan_path: impl AsRef<Path>,
    input_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<MaterializationReceipt> {
    let plan_path = plan_path.as_ref();
    let (plan, plan_sha256): (GenerationPlan, String) = read_contract_with_hash(plan_path)?;
    validate_generation_plan(&plan)?;
    let input_path = input_path.as_ref();
    let input: MaterializationResultInput = read_contract(input_path)?;
    require_schema(&input.schema, MATERIALIZATION_INPUT_SCHEMA)?;
    if input.generation_plan_sha256 != plan_sha256 {
        bail!("materialization result generation plan hash is stale");
    }
    if input.production_manifest_sha256 != plan.production_manifest_sha256 {
        bail!("materialization result production manifest hash does not match plan");
    }
    let expected = plan
        .units
        .iter()
        .map(|unit| (unit.unit_id.as_str(), unit))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut outputs = Vec::new();
    for output in input.outputs {
        let unit = expected.get(output.unit_id.as_str()).ok_or_else(|| {
            anyhow!(
                "materialization result references unknown unit {}",
                output.unit_id
            )
        })?;
        if !seen.insert(output.unit_id.clone()) {
            bail!("duplicate materialized unit {}", output.unit_id);
        }
        require_hash(&output.sha256)?;
        let output_path = resolve_relative_to(input_path, &output.path);
        let output_bytes = fs::read(&output_path)?;
        let actual_sha256 = hash_bytes(&output_bytes);
        if output.sha256 != actual_sha256 {
            bail!("materialized unit {} output hash mismatch", output.unit_id);
        }
        let actual_bytes = output_bytes.len() as u64;
        if output.bytes != actual_bytes {
            bail!("materialized unit {} byte count mismatch", output.unit_id);
        }
        let image = image::load_from_memory(&output_bytes)
            .with_context(|| format!("failed to read dimensions for unit {}", output.unit_id))?;
        let (actual_width, actual_height) = (image.width(), image.height());
        let actual_media_type = detected_image_media_type_bytes(&output_bytes)?;
        if actual_media_type != unit.expected_output.media_type {
            bail!(
                "materialized unit {} media type does not match plan",
                output.unit_id
            );
        }
        if output.width != actual_width
            || output.height != actual_height
            || output.width != unit.expected_output.width
            || output.height != unit.expected_output.height
        {
            bail!(
                "materialized unit {} dimensions do not match plan or file",
                output.unit_id
            );
        }
        outputs.push(MaterializedUnit {
            unit_id: output.unit_id,
            shot_id: unit.shot_id.clone(),
            sha256: actual_sha256,
            bytes: actual_bytes,
            width: actual_width,
            height: actual_height,
            media_type: unit.expected_output.media_type.clone(),
        });
    }
    if seen.len() != expected.len() {
        let missing = expected
            .keys()
            .filter(|unit| !seen.contains(**unit))
            .copied()
            .collect::<Vec<_>>();
        bail!(
            "materialization result is missing units: {}",
            missing.join(",")
        );
    }
    outputs.sort_by(|left, right| left.unit_id.cmp(&right.unit_id));
    let receipt = MaterializationReceipt {
        schema: MATERIALIZATION_RECEIPT_SCHEMA.to_string(),
        generation_plan_sha256: plan_sha256,
        production_manifest_sha256: plan.production_manifest_sha256,
        outputs,
        all_outputs_verified: true,
        provider_executed_by_reel: false,
    };
    write_json_new(&receipt, output_path.as_ref())?;
    Ok(receipt)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromotionState {
    Candidate,
    Selected,
    Approved,
}

impl PromotionState {
    fn predecessor(self) -> Option<Self> {
        match self {
            Self::Candidate => None,
            Self::Selected => Some(Self::Candidate),
            Self::Approved => Some(Self::Selected),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetPromotionInput {
    pub schema: String,
    pub asset_id: String,
    pub asset: LocalFileHash,
    pub state: PromotionState,
    #[serde(default)]
    pub prior_record: Option<LocalFileHash>,
    #[serde(default)]
    pub prior_chain: Vec<LocalFileHash>,
    pub review_evidence_sha256: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssetPromotionRecord {
    pub schema: String,
    pub asset_id: String,
    pub asset_sha256: String,
    pub state: PromotionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_record_sha256: Option<String>,
    pub review_evidence_sha256: Vec<String>,
    pub publication_approved: bool,
    pub rights_approved: bool,
    pub human_authority_required: bool,
}

pub fn write_asset_promotion(
    input_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<AssetPromotionRecord> {
    let input_path = input_path.as_ref();
    let input: AssetPromotionInput = read_contract(input_path)?;
    require_schema(&input.schema, ASSET_PROMOTION_INPUT_SCHEMA)?;
    require_token("asset_id", &input.asset_id)?;
    verify_local_file_metadata("asset", input_path, &input.asset)?;
    validate_hash_list(
        "review_evidence_sha256",
        &input.review_evidence_sha256,
        true,
    )?;
    let mut prior_chain = Vec::new();
    for prior in &input.prior_chain {
        let prior_file = verify_local_file_hash("prior promotion chain record", input_path, prior)?;
        let record: AssetPromotionRecord = serde_json::from_slice(&prior_file.bytes)?;
        validate_promotion_record(&record)?;
        prior_chain.push((prior.sha256.as_str(), record));
    }
    let prior_record_sha256 = match (input.state.predecessor(), input.prior_record) {
        (None, None) if prior_chain.is_empty() => None,
        (None, None) => bail!("candidate promotion must not cite a prior chain"),
        (None, Some(_)) => bail!("candidate promotion must not cite a prior record"),
        (Some(_), None) => bail!("selected and approved promotions require a prior record"),
        (Some(expected_state), Some(prior)) => {
            let prior_file = verify_local_file_hash("prior promotion record", input_path, &prior)?;
            let record: AssetPromotionRecord = serde_json::from_slice(&prior_file.bytes)?;
            validate_promotion_record(&record)?;
            if record.state != expected_state {
                bail!(
                    "asset promotion transition must be {:?} -> {:?}",
                    expected_state,
                    input.state
                );
            }
            if record.asset_id != input.asset_id || record.asset_sha256 != input.asset.sha256 {
                bail!("asset promotion prior record is stale for the requested asset");
            }
            match input.state {
                PromotionState::Selected if !prior_chain.is_empty() => {
                    bail!("selected promotion must not cite an additional prior chain")
                }
                PromotionState::Approved => {
                    let [(candidate_sha256, candidate)] = prior_chain.as_slice() else {
                        bail!(
                            "approved promotion requires the exact candidate record in prior_chain"
                        );
                    };
                    if candidate.state != PromotionState::Candidate
                        || candidate.asset_id != input.asset_id
                        || candidate.asset_sha256 != input.asset.sha256
                        || record.prior_record_sha256.as_deref() != Some(*candidate_sha256)
                    {
                        bail!("approved promotion predecessor chain is incomplete or stale");
                    }
                }
                PromotionState::Candidate | PromotionState::Selected => {}
            }
            Some(prior.sha256)
        }
    };
    let record = AssetPromotionRecord {
        schema: ASSET_PROMOTION_RECORD_SCHEMA.to_string(),
        asset_id: input.asset_id,
        asset_sha256: input.asset.sha256,
        state: input.state,
        prior_record_sha256,
        review_evidence_sha256: input.review_evidence_sha256,
        publication_approved: false,
        rights_approved: false,
        human_authority_required: true,
    };
    write_json_new(&record, output_path.as_ref())?;
    Ok(record)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PicturePurpose {
    ReviewProxy,
    Delivery,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PictureOutputProfile {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub media_type: String,
    pub purpose: PicturePurpose,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PicturePlanInput {
    pub schema: String,
    pub production_manifest_sha256: String,
    pub tool_version: String,
    pub review_profile: String,
    pub disclosure: String,
    pub output_profile: PictureOutputProfile,
    #[serde(default)]
    pub shots: Vec<PictureRecipeInput>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PictureRecipeInput {
    pub shot_id: String,
    #[serde(default)]
    pub prompt_sha256: Option<String>,
    #[serde(default)]
    pub input_hashes: Vec<NamedHash>,
    pub recipe_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PictureCacheIndex {
    pub schema: String,
    pub production_manifest_sha256: String,
    pub entries: Vec<PictureCacheEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PictureCacheEntry {
    pub shot_id: String,
    pub recipe_key: String,
    pub output_sha256: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub local_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PictureWorkStatus {
    ExactByteReuse,
    RecipeEquivalentRegeneration,
    Render,
    Stale,
    Missing,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PicturePlanReport {
    pub schema: String,
    pub production_manifest_sha256: String,
    pub tool_version: String,
    pub review_profile: String,
    pub disclosure: String,
    pub output_profile: PictureOutputProfile,
    pub shots: Vec<PictureShotPlan>,
    pub exact_byte_reuse: usize,
    pub recipe_equivalent_regeneration: usize,
    pub render: usize,
    pub stale: usize,
    pub missing: usize,
    pub proxy: bool,
    pub delivery_ready: bool,
    pub provider_execution_requested: bool,
    pub approvals_inferred: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PictureShotPlan {
    pub shot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_output_sha256: Option<String>,
    pub status: PictureWorkStatus,
}

pub fn picture_plan(
    manifest_path: impl AsRef<Path>,
    input_path: impl AsRef<Path>,
    prior_index_path: Option<&Path>,
    output_path: Option<&Path>,
) -> Result<PicturePlanReport> {
    let manifest_path = manifest_path.as_ref();
    let loaded = production::load(manifest_path)?;
    production::validate(&loaded)?;
    let input_path = input_path.as_ref();
    let input: PicturePlanInput = read_contract(input_path)?;
    require_schema(&input.schema, PICTURE_PLAN_INPUT_SCHEMA)?;
    let manifest_sha256 = hash_bytes(&loaded.bytes);
    if input.production_manifest_sha256 != manifest_sha256 {
        bail!("picture plan production manifest hash is stale");
    }
    require_text("tool_version", &input.tool_version)?;
    require_text("review_profile", &input.review_profile)?;
    require_text("disclosure", &input.disclosure)?;
    validate_picture_profile(&input.output_profile)?;
    let still_shots = loaded
        .manifest
        .shots
        .iter()
        .filter(|shot| shot.media_kind == MediaKind::Still)
        .map(|shot| shot.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut recipe_inputs = BTreeMap::new();
    for recipe in &input.shots {
        require_token("shot_id", &recipe.shot_id)?;
        if !still_shots.contains(recipe.shot_id.as_str()) {
            bail!(
                "picture recipe references unknown or non-still shot {}",
                recipe.shot_id
            );
        }
        if recipe_inputs
            .insert(recipe.shot_id.as_str(), recipe)
            .is_some()
        {
            bail!("duplicate picture recipe for shot {}", recipe.shot_id);
        }
        if let Some(prompt_sha256) = &recipe.prompt_sha256 {
            require_hash(prompt_sha256)?;
        }
        validate_named_hashes(&recipe.input_hashes)?;
        require_hash(&recipe.recipe_sha256)?;
        if recipe.prompt_sha256.is_none() && recipe.input_hashes.is_empty() {
            bail!(
                "picture recipe {} requires a prompt hash or at least one input hash",
                recipe.shot_id
            );
        }
    }
    let prior = prior_index_path
        .map(read_contract::<PictureCacheIndex>)
        .transpose()?;
    if let Some(index) = &prior {
        require_schema(&index.schema, PICTURE_CACHE_INDEX_SCHEMA)?;
    }
    let mut prior_entries = BTreeMap::new();
    if let Some(index) = &prior {
        for entry in &index.entries {
            require_token("cache shot_id", &entry.shot_id)?;
            require_hash(&entry.recipe_key)?;
            require_hash(&entry.output_sha256)?;
            if prior_entries
                .insert(entry.shot_id.as_str(), entry)
                .is_some()
            {
                bail!("duplicate picture cache entry for shot {}", entry.shot_id);
            }
        }
    }
    let prior_manifest_matches = prior
        .as_ref()
        .is_none_or(|index| index.production_manifest_sha256 == manifest_sha256);
    let mut shots = Vec::new();
    for shot in &loaded.manifest.shots {
        if shot.media_kind != MediaKind::Still {
            continue;
        }
        let Some(recipe) = recipe_inputs.get(shot.id.as_str()) else {
            shots.push(PictureShotPlan {
                shot_id: shot.id.clone(),
                recipe_key: None,
                prior_output_sha256: prior_entries
                    .get(shot.id.as_str())
                    .map(|entry| entry.output_sha256.clone()),
                status: PictureWorkStatus::Missing,
            });
            continue;
        };
        let recipe_key = picture_recipe_key(&manifest_sha256, recipe, &input)?;
        let prior_entry = prior_entries.get(shot.id.as_str()).copied();
        let status = match prior_entry {
            None => PictureWorkStatus::Render,
            Some(entry)
                if !prior_manifest_matches
                    || entry.recipe_key != recipe_key
                    || entry.width != input.output_profile.width
                    || entry.height != input.output_profile.height =>
            {
                PictureWorkStatus::Stale
            }
            Some(entry) => match &entry.local_path {
                None => PictureWorkStatus::RecipeEquivalentRegeneration,
                Some(path)
                    if cache_entry_matches(
                        &resolve_relative_to(prior_index_path.expect("prior index exists"), path),
                        entry,
                        &input.output_profile.media_type,
                    ) =>
                {
                    PictureWorkStatus::ExactByteReuse
                }
                Some(_) => PictureWorkStatus::Stale,
            },
        };
        shots.push(PictureShotPlan {
            shot_id: shot.id.clone(),
            recipe_key: Some(recipe_key),
            prior_output_sha256: prior_entry.map(|entry| entry.output_sha256.clone()),
            status,
        });
    }
    let count = |status| shots.iter().filter(|shot| shot.status == status).count();
    let exact_byte_reuse = count(PictureWorkStatus::ExactByteReuse);
    let recipe_equivalent_regeneration = count(PictureWorkStatus::RecipeEquivalentRegeneration);
    let render = count(PictureWorkStatus::Render);
    let stale = count(PictureWorkStatus::Stale);
    let missing = count(PictureWorkStatus::Missing);
    let proxy = input.output_profile.purpose == PicturePurpose::ReviewProxy;
    let report = PicturePlanReport {
        schema: PICTURE_PLAN_SCHEMA.to_string(),
        production_manifest_sha256: manifest_sha256,
        tool_version: input.tool_version,
        review_profile: input.review_profile,
        disclosure: input.disclosure,
        output_profile: input.output_profile,
        shots,
        exact_byte_reuse,
        recipe_equivalent_regeneration,
        render,
        stale,
        missing,
        proxy,
        delivery_ready: !proxy
            && render == 0
            && recipe_equivalent_regeneration == 0
            && stale == 0
            && missing == 0,
        provider_execution_requested: false,
        approvals_inferred: false,
    };
    if let Some(output_path) = output_path {
        write_json_new(&report, output_path)?;
    }
    Ok(report)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingSeverity {
    Blocker,
    Critical,
    Major,
    Minor,
    Note,
}

impl FindingSeverity {
    fn rank(self) -> u8 {
        match self {
            Self::Blocker => 0,
            Self::Critical => 1,
            Self::Major => 2,
            Self::Minor => 3,
            Self::Note => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingStatus {
    Open,
    InProgress,
    Resolved,
    Waived,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimecodedReviewInput {
    pub schema: String,
    pub production_manifest_sha256: String,
    pub findings: Vec<TimecodedFinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimecodedFinding {
    pub id: String,
    pub shot_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub severity: FindingSeverity,
    pub owner: String,
    pub status: FindingStatus,
    pub evidence_sha256: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepairQueueReport {
    pub schema: String,
    pub production_manifest_sha256: String,
    pub open_findings: Vec<RepairQueueItem>,
    pub open_count: usize,
    pub human_decision_required: bool,
    pub approvals_inferred: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepairQueueItem {
    pub finding_id: String,
    pub shot_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub severity: FindingSeverity,
    pub owner: String,
    pub status: FindingStatus,
    pub evidence_sha256: Vec<String>,
}

pub fn repair_queue(
    manifest_path: impl AsRef<Path>,
    findings_path: impl AsRef<Path>,
    output_path: Option<&Path>,
) -> Result<RepairQueueReport> {
    let manifest_path = manifest_path.as_ref();
    let loaded = production::load(manifest_path)?;
    production::validate(&loaded)?;
    let input: TimecodedReviewInput = read_contract(findings_path.as_ref())?;
    require_schema(&input.schema, REVIEW_FINDINGS_SCHEMA)?;
    let manifest_sha256 = hash_bytes(&loaded.bytes);
    if input.production_manifest_sha256 != manifest_sha256 {
        bail!("timecoded review findings production manifest hash is stale");
    }
    let shot_ranges = loaded
        .manifest
        .shots
        .iter()
        .enumerate()
        .map(|(index, shot)| {
            let start = timed_ms(shot.start_seconds, "shot start", &shot.id)?;
            let duration = timed_ms(shot.duration_seconds, "shot duration", &shot.id)?;
            Ok((shot.id.as_str(), (index, start, start + duration)))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut ids = BTreeSet::new();
    let mut open_findings = Vec::new();
    for finding in input.findings {
        require_token("finding id", &finding.id)?;
        require_text("finding owner", &finding.owner)?;
        validate_hash_list("finding evidence_sha256", &finding.evidence_sha256, true)?;
        if !ids.insert(finding.id.clone()) {
            bail!("duplicate timecoded finding {}", finding.id);
        }
        let (_, shot_start, shot_end) =
            shot_ranges.get(finding.shot_id.as_str()).ok_or_else(|| {
                anyhow!(
                    "finding {} references unknown shot {}",
                    finding.id,
                    finding.shot_id
                )
            })?;
        if finding.end_ms <= finding.start_ms
            || finding.start_ms < *shot_start
            || finding.end_ms > *shot_end
        {
            bail!(
                "finding {} has an invalid or out-of-shot time range",
                finding.id
            );
        }
        if matches!(
            finding.status,
            FindingStatus::Open | FindingStatus::InProgress
        ) {
            open_findings.push(RepairQueueItem {
                finding_id: finding.id,
                shot_id: finding.shot_id,
                start_ms: finding.start_ms,
                end_ms: finding.end_ms,
                severity: finding.severity,
                owner: finding.owner,
                status: finding.status,
                evidence_sha256: finding.evidence_sha256,
            });
        }
    }
    open_findings.sort_by_key(|finding| {
        (
            finding.severity.rank(),
            shot_ranges[finding.shot_id.as_str()].0,
            finding.start_ms,
            finding.finding_id.clone(),
        )
    });
    let report = RepairQueueReport {
        schema: REPAIR_QUEUE_SCHEMA.to_string(),
        production_manifest_sha256: manifest_sha256,
        open_count: open_findings.len(),
        open_findings,
        human_decision_required: true,
        approvals_inferred: false,
    };
    if let Some(output_path) = output_path {
        write_json_new(&report, output_path)?;
    }
    Ok(report)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionStateIndex {
    pub schema: String,
    pub manifests: Vec<ProductionStateIndexEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionStateIndexEntry {
    pub id: String,
    pub manifest_path: PathBuf,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionStateAuditReport {
    pub schema: String,
    pub manifests: Vec<ProductionStateAuditItem>,
    pub total: usize,
    pub valid: usize,
    pub timing_ready: usize,
    pub generation_ready: usize,
    pub asset_ready: usize,
    pub preview_ready: usize,
    pub delivery_ready: usize,
    pub stale_hashes: usize,
    pub blocker_count: usize,
    pub approvals_inferred: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionStateAuditItem {
    pub id: String,
    pub manifest_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_sha256: Option<String>,
    pub stale_hash: bool,
    pub valid: bool,
    pub timing_ready: bool,
    pub generation_ready: bool,
    pub asset_ready: bool,
    pub preview_ready: bool,
    pub delivery_ready: bool,
    pub blockers: Vec<String>,
}

pub fn production_state_audit(
    index_path: impl AsRef<Path>,
    output_path: Option<&Path>,
) -> Result<ProductionStateAuditReport> {
    let index_path = index_path.as_ref();
    let index: ProductionStateIndex = read_contract(index_path)?;
    require_schema(&index.schema, PRODUCTION_STATE_INDEX_SCHEMA)?;
    if index.manifests.is_empty() {
        bail!("production state index requires at least one manifest");
    }
    let mut ids = BTreeSet::new();
    let mut manifests = Vec::new();
    for entry in index.manifests {
        require_token("manifest id", &entry.id)?;
        require_hash(&entry.manifest_sha256)?;
        if !ids.insert(entry.id.clone()) {
            bail!("duplicate production state manifest id {}", entry.id);
        }
        let manifest_path = if entry.manifest_path.is_absolute() {
            entry.manifest_path.clone()
        } else {
            index_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&entry.manifest_path)
        };
        let manifest_bytes = fs::read(&manifest_path).ok();
        let actual_sha256 = manifest_bytes.as_deref().map(hash_bytes);
        if actual_sha256.as_deref() != Some(entry.manifest_sha256.as_str()) {
            manifests.push(ProductionStateAuditItem {
                id: entry.id,
                manifest_sha256: entry.manifest_sha256,
                actual_sha256,
                stale_hash: true,
                valid: false,
                timing_ready: false,
                generation_ready: false,
                asset_ready: false,
                preview_ready: false,
                delivery_ready: false,
                blockers: vec!["stale-or-unreadable-manifest-hash".to_string()],
            });
            continue;
        }
        let loaded = manifest_bytes
            .and_then(|bytes| {
                serde_yaml::from_slice(&bytes).ok().map(|manifest| {
                    production::LoadedProductionManifest {
                        path: manifest_path,
                        manifest,
                        bytes,
                    }
                })
            })
            .ok_or_else(|| anyhow!("manifest parsing failed"));
        match loaded.and_then(|loaded| production::validate(&loaded)) {
            Ok(validation) => manifests.push(ProductionStateAuditItem {
                id: entry.id,
                manifest_sha256: entry.manifest_sha256,
                actual_sha256,
                stale_hash: false,
                valid: true,
                timing_ready: validation.timing_ready,
                generation_ready: validation.generation_ready,
                asset_ready: validation.asset_ready,
                preview_ready: validation.preview_ready,
                delivery_ready: validation.delivery_ready,
                blockers: validation.semantic_blockers,
            }),
            Err(_) => manifests.push(ProductionStateAuditItem {
                id: entry.id,
                manifest_sha256: entry.manifest_sha256,
                actual_sha256,
                stale_hash: false,
                valid: false,
                timing_ready: false,
                generation_ready: false,
                asset_ready: false,
                preview_ready: false,
                delivery_ready: false,
                blockers: vec!["manifest-validation-failed".to_string()],
            }),
        }
    }
    manifests.sort_by(|left, right| left.id.cmp(&right.id));
    let blocker_count = manifests.iter().map(|item| item.blockers.len()).sum();
    let report = ProductionStateAuditReport {
        schema: PRODUCTION_STATE_AUDIT_SCHEMA.to_string(),
        total: manifests.len(),
        valid: manifests.iter().filter(|item| item.valid).count(),
        timing_ready: manifests.iter().filter(|item| item.timing_ready).count(),
        generation_ready: manifests
            .iter()
            .filter(|item| item.generation_ready)
            .count(),
        asset_ready: manifests.iter().filter(|item| item.asset_ready).count(),
        preview_ready: manifests.iter().filter(|item| item.preview_ready).count(),
        delivery_ready: manifests.iter().filter(|item| item.delivery_ready).count(),
        stale_hashes: manifests.iter().filter(|item| item.stale_hash).count(),
        blocker_count,
        manifests,
        approvals_inferred: false,
    };
    if let Some(output_path) = output_path {
        write_json_new(&report, output_path)?;
    }
    Ok(report)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TakeDisposition {
    Available,
    Rejected,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceTakeLedgerInput {
    pub schema: String,
    pub production_manifest_sha256: String,
    pub voice_plan: LocalFileHash,
    #[serde(default)]
    pub takes: Vec<VoiceTakeInput>,
    #[serde(default)]
    pub selections: Vec<VoiceTakeSelection>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceTakeInput {
    pub cue_id: String,
    pub take_id: String,
    pub audio: LocalFileHash,
    pub start_ms: u64,
    pub end_ms: u64,
    pub disposition: TakeDisposition,
    pub evidence_sha256: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceTakeSelection {
    pub cue_id: String,
    pub take_id: String,
    pub evidence_sha256: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceTakeReport {
    pub schema: String,
    pub production_manifest_sha256: String,
    pub voice_plan_sha256: String,
    pub takes: Vec<VoiceTakeRecord>,
    pub selected_takes: BTreeMap<String, String>,
    pub retake_queue: Vec<VoiceRetakeItem>,
    pub awaiting_selection: Vec<String>,
    pub synthesis_requested: bool,
    pub voice_approval_inferred: bool,
    pub human_authority_required: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceTakeRecord {
    pub cue_id: String,
    pub take_id: String,
    pub audio_sha256: String,
    pub bytes: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub disposition: TakeDisposition,
    pub selected: bool,
    pub evidence_sha256: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetakeReason {
    Missing,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceRetakeItem {
    pub cue_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub reason: RetakeReason,
}

pub fn voice_take_ledger(
    manifest_path: impl AsRef<Path>,
    input_path: impl AsRef<Path>,
    output_path: Option<&Path>,
) -> Result<VoiceTakeReport> {
    let manifest_path = manifest_path.as_ref();
    let loaded = production::load(manifest_path)?;
    let validation = production::validate(&loaded)?;
    let input_path = input_path.as_ref();
    let input: VoiceTakeLedgerInput = read_contract(input_path)?;
    require_schema(&input.schema, VOICE_TAKE_INPUT_SCHEMA)?;
    let manifest_sha256 = hash_bytes(&loaded.bytes);
    if input.production_manifest_sha256 != manifest_sha256 {
        bail!("voice take ledger production manifest hash is stale");
    }
    verify_local_file_metadata("voice plan", input_path, &input.voice_plan)?;
    let cue_ranges = loaded
        .manifest
        .narration_cues
        .iter()
        .map(|cue| {
            let start = timed_ms(cue.start_seconds, "cue start", &cue.id)?;
            let duration = timed_ms(cue.duration_seconds, "cue duration", &cue.id)?;
            if duration == 0 {
                bail!("cue duration must be positive for {}", cue.id);
            }
            let end = start
                .checked_add(duration)
                .ok_or_else(|| anyhow!("cue range overflows for {}", cue.id))?;
            if validation
                .duration_ms
                .is_some_and(|timeline| end > timeline)
            {
                bail!("cue range exceeds production timeline for {}", cue.id);
            }
            Ok((cue.id.as_str(), (start, end)))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut take_keys = BTreeSet::new();
    let mut takes_by_cue: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut takes = Vec::new();
    for take in input.takes {
        require_token("cue_id", &take.cue_id)?;
        require_token("take_id", &take.take_id)?;
        validate_hash_list("take evidence_sha256", &take.evidence_sha256, true)?;
        let expected_range = cue_ranges
            .get(take.cue_id.as_str())
            .ok_or_else(|| anyhow!("voice take references unknown cue {}", take.cue_id))?;
        if (take.start_ms, take.end_ms) != *expected_range {
            bail!(
                "voice take {} does not match the exact cue span",
                take.take_id
            );
        }
        if !take_keys.insert((take.cue_id.clone(), take.take_id.clone())) {
            bail!(
                "duplicate voice take {} for cue {}",
                take.take_id,
                take.cue_id
            );
        }
        let take_audio = verify_local_file_metadata("voice take audio", input_path, &take.audio)?;
        let index = takes.len();
        takes_by_cue
            .entry(take.cue_id.clone())
            .or_default()
            .push(index);
        takes.push(VoiceTakeRecord {
            cue_id: take.cue_id,
            take_id: take.take_id,
            audio_sha256: take.audio.sha256,
            bytes: take_audio.bytes,
            start_ms: take.start_ms,
            end_ms: take.end_ms,
            disposition: take.disposition,
            selected: false,
            evidence_sha256: take.evidence_sha256,
        });
    }
    let mut selected_takes = BTreeMap::new();
    for selection in input.selections {
        validate_hash_list(
            "selection evidence_sha256",
            &selection.evidence_sha256,
            true,
        )?;
        if selected_takes
            .insert(selection.cue_id.clone(), selection.take_id.clone())
            .is_some()
        {
            bail!(
                "cue {} has multiple explicit take selections",
                selection.cue_id
            );
        }
        let take = takes
            .iter_mut()
            .find(|take| take.cue_id == selection.cue_id && take.take_id == selection.take_id)
            .ok_or_else(|| {
                anyhow!(
                    "selection references unknown take {} for cue {}",
                    selection.take_id,
                    selection.cue_id
                )
            })?;
        if take.disposition == TakeDisposition::Rejected {
            bail!("rejected take {} cannot be selected", selection.take_id);
        }
        take.selected = true;
        take.evidence_sha256.extend(selection.evidence_sha256);
        take.evidence_sha256.sort();
        take.evidence_sha256.dedup();
    }
    let mut retake_queue = Vec::new();
    let mut awaiting_selection = Vec::new();
    for cue in &loaded.manifest.narration_cues {
        if selected_takes.contains_key(&cue.id) {
            continue;
        }
        let (start_ms, end_ms) = cue_ranges[cue.id.as_str()];
        let cue_takes = takes_by_cue.get(&cue.id);
        match cue_takes {
            None => retake_queue.push(VoiceRetakeItem {
                cue_id: cue.id.clone(),
                start_ms,
                end_ms,
                reason: RetakeReason::Missing,
            }),
            Some(indexes)
                if indexes
                    .iter()
                    .all(|index| takes[*index].disposition == TakeDisposition::Rejected) =>
            {
                retake_queue.push(VoiceRetakeItem {
                    cue_id: cue.id.clone(),
                    start_ms,
                    end_ms,
                    reason: RetakeReason::Rejected,
                });
            }
            Some(_) => awaiting_selection.push(cue.id.clone()),
        }
    }
    takes
        .sort_by(|left, right| (&left.cue_id, &left.take_id).cmp(&(&right.cue_id, &right.take_id)));
    retake_queue.sort_by_key(|item| (item.start_ms, item.cue_id.clone()));
    awaiting_selection.sort();
    let report = VoiceTakeReport {
        schema: VOICE_TAKE_REPORT_SCHEMA.to_string(),
        production_manifest_sha256: manifest_sha256,
        voice_plan_sha256: input.voice_plan.sha256,
        takes,
        selected_takes,
        retake_queue,
        awaiting_selection,
        synthesis_requested: false,
        voice_approval_inferred: false,
        human_authority_required: true,
    };
    if let Some(output_path) = output_path {
        write_json_new(&report, output_path)?;
    }
    Ok(report)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MusicVariantKind {
    Scored,
    NoScore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MusicSourceClassification {
    OriginalCommission,
    LicensedLibrary,
    PublicDomain,
    NoScore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MusicLicenseClassification {
    Unknown,
    Documented,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MusicProvenanceClassification {
    HumanAuthored,
    ToolAssisted,
    Generated,
    Mixed,
    NoScore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MusicReviewStatus {
    Pending,
    Reviewed,
    ChangesRequested,
    Rejected,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicProvenanceInput {
    pub schema: String,
    pub production_manifest_sha256: String,
    #[serde(default)]
    pub score_plan: Option<LocalFileHash>,
    pub variants: Vec<MusicVariantInput>,
    #[serde(default)]
    pub comparison: Option<MusicComparisonClaim>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MusicVariantInput {
    pub id: String,
    pub kind: MusicVariantKind,
    pub audio: LocalFileHash,
    pub source: MusicSourceClassification,
    pub license: MusicLicenseClassification,
    pub provenance: MusicProvenanceClassification,
    pub human_review_status: MusicReviewStatus,
    pub evidence_sha256: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MusicComparisonClaim {
    pub scored_variant_sha256: String,
    pub no_score_variant_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MusicProvenanceReport {
    pub schema: String,
    pub production_manifest_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_plan_sha256: Option<String>,
    pub variants: Vec<MusicVariantRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<MusicComparisonClaim>,
    pub comparison_verified: bool,
    pub rights_approval_inferred: bool,
    pub creative_approval_inferred: bool,
    pub human_authority_required: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MusicVariantRecord {
    pub id: String,
    pub kind: MusicVariantKind,
    pub audio_sha256: String,
    pub bytes: u64,
    pub source: MusicSourceClassification,
    pub license: MusicLicenseClassification,
    pub provenance: MusicProvenanceClassification,
    pub human_review_status: MusicReviewStatus,
    pub evidence_sha256: Vec<String>,
}

pub fn music_provenance(
    manifest_path: impl AsRef<Path>,
    input_path: impl AsRef<Path>,
    output_path: Option<&Path>,
) -> Result<MusicProvenanceReport> {
    let manifest_path = manifest_path.as_ref();
    let loaded = production::load(manifest_path)?;
    production::validate(&loaded)?;
    let input_path = input_path.as_ref();
    let input: MusicProvenanceInput = read_contract(input_path)?;
    require_schema(&input.schema, MUSIC_PROVENANCE_INPUT_SCHEMA)?;
    let manifest_sha256 = hash_bytes(&loaded.bytes);
    if input.production_manifest_sha256 != manifest_sha256 {
        bail!("music provenance production manifest hash is stale");
    }
    if input.variants.is_empty() {
        bail!("music provenance requires at least one exact variant");
    }
    if let Some(score_plan) = &input.score_plan {
        verify_local_file_metadata("score plan", input_path, score_plan)?;
    }
    let mut ids = BTreeSet::new();
    let mut audio_hashes = BTreeSet::new();
    let mut variants = Vec::new();
    for variant in input.variants {
        require_token("music variant id", &variant.id)?;
        validate_hash_list("music evidence_sha256", &variant.evidence_sha256, true)?;
        if !ids.insert(variant.id.clone()) {
            bail!("duplicate music variant {}", variant.id);
        }
        let audio = verify_local_file_metadata("music variant audio", input_path, &variant.audio)?;
        if !audio_hashes.insert(variant.audio.sha256.clone()) {
            bail!("music variants must bind distinct exact audio hashes");
        }
        match variant.kind {
            MusicVariantKind::Scored => {
                if input.score_plan.is_none() {
                    bail!("scored music variant requires an exact score plan");
                }
                if variant.source == MusicSourceClassification::NoScore
                    || variant.license == MusicLicenseClassification::NotApplicable
                    || variant.provenance == MusicProvenanceClassification::NoScore
                {
                    bail!("scored variant uses no-score provenance classifications");
                }
            }
            MusicVariantKind::NoScore => {
                if variant.source != MusicSourceClassification::NoScore
                    || variant.license != MusicLicenseClassification::NotApplicable
                    || variant.provenance != MusicProvenanceClassification::NoScore
                {
                    bail!("no-score variant must use explicit no-score classifications");
                }
            }
        }
        variants.push(MusicVariantRecord {
            id: variant.id,
            kind: variant.kind,
            audio_sha256: variant.audio.sha256,
            bytes: audio.bytes,
            source: variant.source,
            license: variant.license,
            provenance: variant.provenance,
            human_review_status: variant.human_review_status,
            evidence_sha256: variant.evidence_sha256,
        });
    }
    let comparison_verified = match &input.comparison {
        None => false,
        Some(claim) => {
            require_hash(&claim.scored_variant_sha256)?;
            require_hash(&claim.no_score_variant_sha256)?;
            let scored = variants.iter().any(|variant| {
                variant.kind == MusicVariantKind::Scored
                    && variant.audio_sha256 == claim.scored_variant_sha256
            });
            let no_score = variants.iter().any(|variant| {
                variant.kind == MusicVariantKind::NoScore
                    && variant.audio_sha256 == claim.no_score_variant_sha256
            });
            if !scored || !no_score {
                bail!(
                    "score/no-score comparison requires both exact scored and no-score variant hashes"
                );
            }
            true
        }
    };
    variants.sort_by(|left, right| left.id.cmp(&right.id));
    let report = MusicProvenanceReport {
        schema: MUSIC_PROVENANCE_REPORT_SCHEMA.to_string(),
        production_manifest_sha256: manifest_sha256,
        score_plan_sha256: input.score_plan.map(|plan| plan.sha256),
        variants,
        comparison: input.comparison,
        comparison_verified,
        rights_approval_inferred: false,
        creative_approval_inferred: false,
        human_authority_required: true,
    };
    if let Some(output_path) = output_path {
        write_json_new(&report, output_path)?;
    }
    Ok(report)
}

fn read_contract<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    let path = path.as_ref();
    serde_yaml::from_slice(&fs::read(path).with_context(|| {
        format!(
            "failed to read production operations contract {}",
            path.display()
        )
    })?)
    .with_context(|| {
        format!(
            "failed to parse production operations contract {}",
            path.display()
        )
    })
}

fn read_contract_with_hash<T: DeserializeOwned>(path: &Path) -> Result<(T, String)> {
    let bytes = fs::read(path).with_context(|| {
        format!(
            "failed to read production operations contract {}",
            path.display()
        )
    })?;
    let value = serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "failed to parse production operations contract {}",
            path.display()
        )
    })?;
    Ok((value, hash_bytes(&bytes)))
}

fn write_json_new<T: Serialize>(value: &T, output: &Path) -> Result<()> {
    if output.exists() {
        bail!("refusing to overwrite {}", output.display());
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(format!("{}\n", serde_json::to_string_pretty(value)?).as_bytes())?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output.display()))?;
    Ok(())
}

fn require_schema(actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("unsupported schema {actual}; expected {expected}");
    }
    Ok(())
}

fn require_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn require_token(field: &str, value: &str) -> Result<()> {
    require_text(field, value)?;
    if value.contains('/') || value.contains('\\') || value == "." || value == ".." {
        bail!("{field} must be a portable identifier");
    }
    Ok(())
}

fn require_hash(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("expected a lowercase SHA-256 digest");
    }
    Ok(())
}

fn validate_hash_list(field: &str, values: &[String], required: bool) -> Result<()> {
    if required && values.is_empty() {
        bail!("{field} requires at least one hash");
    }
    let mut unique = BTreeSet::new();
    for value in values {
        require_hash(value)?;
        if !unique.insert(value) {
            bail!("{field} contains a duplicate hash");
        }
    }
    Ok(())
}

fn validate_named_hashes(values: &[NamedHash]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for value in values {
        require_token("input hash id", &value.id)?;
        require_hash(&value.sha256)?;
        if !ids.insert(value.id.as_str()) {
            bail!("duplicate input hash id {}", value.id);
        }
    }
    Ok(())
}

fn validate_visual_output(output: &ExpectedVisualOutput) -> Result<()> {
    require_supported_image_media_type(&output.media_type)?;
    if output.width == 0 || output.height == 0 {
        bail!("expected output dimensions must be positive");
    }
    Ok(())
}

struct VerifiedLocalFile {
    bytes: Vec<u8>,
}

struct VerifiedLocalFileMetadata {
    bytes: u64,
}

fn verify_local_file_hash(
    label: &str,
    contract_path: &Path,
    binding: &LocalFileHash,
) -> Result<VerifiedLocalFile> {
    require_hash(&binding.sha256)?;
    let path = resolve_relative_to(contract_path, &binding.path);
    if fs::metadata(&path)?.len() > 16 * 1024 * 1024 {
        bail!("{label} exceeds the 16 MiB contract size limit");
    }
    let bytes = fs::read(&path).with_context(|| format!("failed to read {label}"))?;
    let actual = hash_bytes(&bytes);
    if binding.sha256 != actual {
        bail!("{label} hash mismatch");
    }
    Ok(VerifiedLocalFile { bytes })
}

fn verify_local_file_metadata(
    label: &str,
    contract_path: &Path,
    binding: &LocalFileHash,
) -> Result<VerifiedLocalFileMetadata> {
    require_hash(&binding.sha256)?;
    let path = resolve_relative_to(contract_path, &binding.path);
    let mut file = fs::File::open(&path).with_context(|| format!("failed to read {label}"))?;
    let mut hasher = Sha256::new();
    let bytes = std::io::copy(&mut file, &mut HashWriter(&mut hasher))?;
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if binding.sha256 != actual {
        bail!("{label} hash mismatch");
    }
    Ok(VerifiedLocalFileMetadata { bytes })
}

struct HashWriter<'a>(&'a mut Sha256);

impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn validate_picture_profile(profile: &PictureOutputProfile) -> Result<()> {
    require_token("output profile id", &profile.id)?;
    require_supported_image_media_type(&profile.media_type)?;
    if profile.width == 0 || profile.height == 0 {
        bail!("picture output profile dimensions must be positive");
    }
    Ok(())
}

fn picture_recipe_key(
    manifest_sha256: &str,
    recipe: &PictureRecipeInput,
    input: &PicturePlanInput,
) -> Result<String> {
    let value = serde_json::json!({
        "production_manifest_sha256": manifest_sha256,
        "shot_id": recipe.shot_id,
        "prompt_sha256": recipe.prompt_sha256,
        "input_hashes": recipe.input_hashes,
        "recipe_sha256": recipe.recipe_sha256,
        "output_profile": input.output_profile,
        "tool_version": input.tool_version,
        "review_profile": input.review_profile,
        "disclosure": input.disclosure,
    });
    Ok(hash_bytes(&serde_json::to_vec(&value)?))
}

fn cache_entry_matches(path: &Path, entry: &PictureCacheEntry, media_type: &str) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let actual_sha256 = hash_bytes(&bytes);
    let Ok(image) = image::load_from_memory(&bytes) else {
        return false;
    };
    let Ok(actual_media_type) = detected_image_media_type_bytes(&bytes) else {
        return false;
    };
    actual_sha256 == entry.output_sha256
        && bytes.len() as u64 == entry.bytes
        && image.width() == entry.width
        && image.height() == entry.height
        && actual_media_type == media_type
}

fn require_supported_image_media_type(media_type: &str) -> Result<()> {
    if media_type != "image/png" {
        bail!("unsupported visual media type {media_type}; expected image/png");
    }
    Ok(())
}

fn detected_image_media_type_bytes(bytes: &[u8]) -> Result<&'static str> {
    match image::guess_format(bytes)? {
        image::ImageFormat::Png => Ok("image/png"),
        format => bail!("unsupported materialized image format {format:?}; expected PNG"),
    }
}

fn resolve_relative_to(contract_path: &Path, embedded_path: &Path) -> PathBuf {
    if embedded_path.is_absolute() {
        embedded_path.to_path_buf()
    } else {
        contract_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(embedded_path)
    }
}

fn validate_generation_plan(plan: &GenerationPlan) -> Result<()> {
    require_schema(&plan.schema, GENERATION_PLAN_SCHEMA)?;
    require_hash(&plan.source_contract_sha256)?;
    require_hash(&plan.production_manifest_sha256)?;
    require_text("generation plan tool_version", &plan.tool_version)?;
    if plan.provider_execution_requested {
        bail!("generation plan must not request provider execution");
    }
    if plan.units.is_empty() {
        bail!("generation plan requires at least one unit");
    }
    let mut units = BTreeSet::new();
    let mut shots = BTreeSet::new();
    for unit in &plan.units {
        require_token("generation unit_id", &unit.unit_id)?;
        require_token("generation shot_id", &unit.shot_id)?;
        if !units.insert(unit.unit_id.as_str()) || !shots.insert(unit.shot_id.as_str()) {
            bail!("generation plan contains duplicate unit or shot bindings");
        }
        require_hash(&unit.prompt_sha256)?;
        validate_named_hashes(&unit.input_hashes)?;
        validate_visual_output(&unit.expected_output)?;
    }
    Ok(())
}

fn validate_promotion_record(record: &AssetPromotionRecord) -> Result<()> {
    require_schema(&record.schema, ASSET_PROMOTION_RECORD_SCHEMA)?;
    require_token("promotion asset_id", &record.asset_id)?;
    require_hash(&record.asset_sha256)?;
    validate_hash_list(
        "promotion review_evidence_sha256",
        &record.review_evidence_sha256,
        true,
    )?;
    match record.state {
        PromotionState::Candidate if record.prior_record_sha256.is_some() => {
            bail!("candidate promotion record must not cite a predecessor")
        }
        PromotionState::Selected | PromotionState::Approved => {
            require_hash(record.prior_record_sha256.as_deref().ok_or_else(|| {
                anyhow!("selected and approved promotion records require a predecessor hash")
            })?)?;
        }
        PromotionState::Candidate => {}
    }
    if record.publication_approved || record.rights_approved || !record.human_authority_required {
        bail!("promotion record violates the human authority boundary");
    }
    Ok(())
}

fn timed_ms(value: Option<f64>, field: &str, id: &str) -> Result<u64> {
    let value = value.ok_or_else(|| anyhow!("{field} is required for {id}"))?;
    if !value.is_finite() || value < 0.0 {
        bail!("{field} is invalid for {id}");
    }
    Ok((value * 1000.0).round() as u64)
}

fn hash_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
