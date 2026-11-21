//! Bounded lossless candidate assembly. No provider, storage promotion or approval.
use super::compiler::{self, CompilationProfile, CompilerIdentity};
use super::*;
use crate::{
    browser_composition::pcm_window::{self, PcmWindow},
    scene_delivery::InputFiles,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const PROCESSING_SCHEMA: &str = "reel.spoken-processing.v1";
pub const RECEIPT_SCHEMA: &str = "reel.spoken-production-receipt.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RationalGain {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CueProcessing {
    pub edition_id: String,
    pub performance_id: String,
    pub gain: RationalGain,
    pub fade_in_samples: u64,
    pub fade_out_samples: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessingPolicy {
    pub schema: String,
    pub operations: Vec<CueProcessing>,
}
impl ProcessingPolicy {
    pub fn identity() -> Self {
        Self {
            schema: PROCESSING_SCHEMA.into(),
            operations: vec![],
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TakeEvidence {
    pub performance_id: String,
    pub source: FileRef,
    pub selected_pcm_sha256: String,
    pub processed_pcm_sha256: String,
    pub processing: CueProcessing,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProducedEdition {
    pub edition_id: String,
    pub compiled_recipe_sha256: String,
    pub execution_recipe_sha256: String,
    pub samples: u64,
    pub wave: FileRef,
    pub pcm_sha256: String,
    pub takes: Vec<TakeEvidence>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionReceipt {
    pub schema: String,
    pub producer: CompilerIdentity,
    pub declaration_semantics_sha256: String,
    pub compilation_profile: CompilationProfile,
    pub editions: Vec<ProducedEdition>,
    pub source_bytes_verified: bool,
    pub producer_structural_checks_passed: bool,
    pub independent_check_passed: bool,
    pub native_admission_verified: bool,
    pub authority_verified: bool,
    pub publication: String,
}

#[derive(Debug, Serialize)]
pub struct ProductionSummary {
    pub schema: &'static str,
    pub editions: usize,
    pub source_bytes_verified: bool,
    pub independent_check_passed: bool,
    pub authority_verified: bool,
    pub publication: &'static str,
}

/// A host may narrow the profile deadline and supply cooperative cancellation.
/// It cannot enlarge the admitted profile's resource or duration limits.
pub struct ProductionControl<F> {
    pub timeout: Duration,
    pub cancellation: F,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn current_runtime() -> Result<CompilerIdentity> {
    let path =
        std::env::current_exe().map_err(|_| anyhow::anyhow!("spoken runtime unavailable"))?;
    Ok(CompilerIdentity {
        sha256: crate::sha256_file(&path)
            .map_err(|_| anyhow::anyhow!("spoken runtime identity unavailable"))?,
        bytes: fs::metadata(path)
            .map_err(|_| anyhow::anyhow!("spoken runtime size unavailable"))?
            .len(),
    })
}

pub fn parse_processing(bytes: &[u8]) -> Result<ProcessingPolicy> {
    require(
        bytes.len() <= MAX_DECLARATION_BYTES,
        "spoken processing exceeds budget",
    )?;
    serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("spoken processing is not strict JSON"))
}

pub(crate) fn operation(
    policy: &ProcessingPolicy,
    edition: &str,
    performance: &str,
) -> CueProcessing {
    policy
        .operations
        .iter()
        .find(|p| p.edition_id == edition && p.performance_id == performance)
        .cloned()
        .unwrap_or(CueProcessing {
            edition_id: edition.into(),
            performance_id: performance.into(),
            gain: RationalGain {
                numerator: 1,
                denominator: 1,
            },
            fade_in_samples: 0,
            fade_out_samples: 0,
        })
}

pub(crate) fn validate_processing(
    policy: &ProcessingPolicy,
    plan: &compiler::CompiledPlan,
) -> Result<()> {
    require(
        policy.schema == PROCESSING_SCHEMA && policy.operations.len() <= 1024,
        "spoken processing schema or count invalid",
    )?;
    let mut seen = BTreeSet::new();
    for op in &policy.operations {
        require(
            seen.insert((&op.edition_id, &op.performance_id)),
            "spoken duplicate processing target",
        )?;
        let cue = plan
            .editions
            .iter()
            .find(|e| e.recipe.edition_id == op.edition_id)
            .and_then(|e| {
                e.recipe
                    .cues
                    .iter()
                    .find(|c| c.performance.id == op.performance_id)
            })
            .context("spoken processing target missing")?;
        require(
            op.gain.denominator > 0
                && op.gain.denominator <= 1_000_000
                && u64::from(op.gain.numerator) <= u64::from(op.gain.denominator) * 8,
            "spoken rational gain invalid",
        )?;
        let span = cue.selection.end_sample - cue.selection.start_sample;
        require(
            op.fade_in_samples <= span && op.fade_out_samples <= span - op.fade_in_samples,
            "spoken envelope bounds invalid",
        )?;
        for region in &cue.performance.protected {
            let hits_in = op.fade_in_samples > 0
                && region.start_sample < cue.selection.start_sample + op.fade_in_samples
                && region.end_sample > cue.selection.start_sample;
            let hits_out = op.fade_out_samples > 0
                && region.start_sample < cue.selection.end_sample
                && region.end_sample > cue.selection.end_sample - op.fade_out_samples;
            require(
                !hits_in && !hits_out,
                "spoken envelope crosses protected interval",
            )?;
        }
    }
    Ok(())
}

fn source_key(source: &FileRef) -> (PathBuf, String, u64) {
    (source.path.clone(), source.sha256.clone(), source.bytes)
}

pub(crate) fn read_source(
    inputs: &(impl InputFiles + ?Sized),
    reference: &FileRef,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    check()?;
    let path = inputs
        .verified_path(reference)
        .map_err(|_| anyhow::anyhow!("spoken source unavailable"))?;
    let mut file = File::open(path).map_err(|_| anyhow::anyhow!("spoken source open failed"))?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        check()?;
        let n = file
            .read(&mut buffer)
            .map_err(|_| anyhow::anyhow!("spoken source read failed"))?;
        if n == 0 {
            break;
        }
        require(
            bytes.len() as u64 + n as u64 <= reference.bytes,
            "spoken source exceeds pin",
        )?;
        bytes.extend_from_slice(&buffer[..n]);
    }
    require(
        bytes.len() as u64 == reference.bytes && digest(&bytes) == reference.sha256,
        "spoken source bytes differ from pin",
    )?;
    Ok(bytes)
}

fn pcm_value(bytes: &[u8]) -> i64 {
    i64::from(i32::from_le_bytes([
        bytes[0],
        bytes[1],
        bytes[2],
        if bytes[2] & 128 != 0 { 255 } else { 0 },
    ]))
}

fn wave_bytes(pcm: &[u8]) -> Result<Vec<u8>> {
    require(
        !pcm.is_empty() && pcm.len() % 6 == 0,
        "spoken PCM frame alignment invalid",
    )?;
    let size = u32::try_from(pcm.len()).context("spoken WAVE size overflow")?;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((size + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(48000u32.to_le_bytes());
    bytes.extend(288000u32.to_le_bytes());
    bytes.extend(6u16.to_le_bytes());
    bytes.extend(24u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    bytes.extend(pcm);
    Ok(bytes)
}

/// Candidate files are not usable without the final receipt. Failure retires only
/// the exact files created here; unrelated files in a reserved directory survive.
pub(crate) struct OutputOwner {
    root: PathBuf,
    names: Vec<PathBuf>,
    complete: bool,
}
impl OutputOwner {
    pub(crate) fn reserve(root: &Path) -> Result<Self> {
        fs::create_dir(root).map_err(|_| anyhow::anyhow!("spoken output unavailable or exists"))?;
        Ok(Self {
            root: root.into(),
            names: vec![],
            complete: false,
        })
    }
    pub(crate) fn finish(mut self) {
        self.complete = true;
    }
}
impl Drop for OutputOwner {
    fn drop(&mut self) {
        if !self.complete {
            for name in self.names.iter().rev() {
                let _ = fs::remove_file(self.root.join(name));
            }
            let _ = fs::remove_dir(&self.root);
        }
    }
}

pub(crate) fn persist_file(
    owner: &mut OutputOwner,
    name: &str,
    bytes: &[u8],
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut staged = tempfile::NamedTempFile::new_in(&owner.root)
        .map_err(|_| anyhow::anyhow!("spoken candidate staging failed"))?;
    staged
        .write_all(bytes)
        .map_err(|_| anyhow::anyhow!("spoken candidate write failed"))?;
    // Check after staging as well: cancellation must prevent the final receipt
    // from becoming visible even when it arrives during its write.
    check()?;
    staged
        .persist_noclobber(owner.root.join(name))
        .map_err(|_| anyhow::anyhow!("spoken candidate name occupied"))?;
    owner.names.push(PathBuf::from(name));
    Ok(())
}

pub fn produce(
    document: &Document,
    plan_bytes: &[u8],
    policy: &ProcessingPolicy,
    inputs: &(impl InputFiles + ?Sized),
    output: &Path,
    cancellation: impl FnMut() -> Result<()>,
) -> Result<ProductionReceipt> {
    produce_controlled(
        document,
        plan_bytes,
        policy,
        inputs,
        output,
        ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation,
        },
    )
}

pub fn produce_controlled(
    document: &Document,
    plan_bytes: &[u8],
    policy: &ProcessingPolicy,
    inputs: &(impl InputFiles + ?Sized),
    output: &Path,
    mut control: ProductionControl<impl FnMut() -> Result<()>>,
) -> Result<ProductionReceipt> {
    require(
        plan_bytes.len() <= 16 * 1024 * 1024,
        "spoken plan exceeds budget",
    )?;
    let raw: serde_json::Value = serde_json::from_slice(plan_bytes)
        .map_err(|_| anyhow::anyhow!("spoken plan JSON invalid"))?;
    let profile: CompilationProfile = serde_json::from_value(
        raw.pointer("/editions/0/recipe/compilation_profile")
            .cloned()
            .context("spoken plan profile missing")?,
    )
    .map_err(|_| anyhow::anyhow!("spoken plan profile unsupported"))?;
    require(
        control.timeout <= Duration::from_secs(profile.timeout_seconds()),
        "spoken deadline exceeds profile",
    )?;
    let started = Instant::now();
    let deadline = started + control.timeout;
    let mut check = || -> Result<()> {
        (control.cancellation)()?;
        require(
            Instant::now() < deadline,
            "spoken production deadline elapsed",
        )
    };
    check()?;
    let runtime = current_runtime()?;
    let plan = compiler::compile_with_inputs(document, inputs, &runtime, profile)?;
    require(
        serde_json::to_vec_pretty(&plan)? == plan_bytes,
        "spoken plan differs from current controls/runtime",
    )?;
    validate_processing(policy, &plan)?;
    let mut sources = BTreeMap::new();
    let mut frames = BTreeMap::new();
    let mut encoded_bytes = 0u64;
    let mut pcm_bytes = 0u64;
    let mut output_bytes = 0u64;
    for edition in &plan.editions {
        output_bytes = output_bytes
            .checked_add(edition.duration_samples * 6 + 44)
            .context("spoken output budget overflow")?;
        for cue in &edition.recipe.cues {
            let key = source_key(&cue.performance.source);
            if let Some(count) = frames.get(&key) {
                require(
                    *count == cue.performance.duration_samples,
                    "spoken source sample declarations disagree",
                )?;
            } else {
                frames.insert(key, cue.performance.duration_samples);
                encoded_bytes = encoded_bytes
                    .checked_add(cue.performance.source.bytes)
                    .context("spoken source budget overflow")?;
                pcm_bytes = pcm_bytes
                    .checked_add(cue.performance.duration_samples * 6)
                    .context("spoken PCM budget overflow")?;
            }
        }
    }
    require(
        encoded_bytes <= profile.max_input_pcm_bytes()
            && pcm_bytes <= profile.max_input_pcm_bytes(),
        "spoken input budget exceeded",
    )?;
    require(
        output_bytes <= profile.max_output_bytes()
            && output_bytes + 16 * 1024 * 1024 <= profile.max_workspace_bytes(),
        "spoken output/workspace budget exceeded",
    )?;
    for (key, count) in &frames {
        let source = FileRef {
            path: key.0.clone(),
            sha256: key.1.clone(),
            bytes: key.2,
        };
        let bytes = read_source(inputs, &source, &mut check)?;
        let pcm = crate::song_edition::wave::pcm24(&bytes)
            .map_err(|_| anyhow::anyhow!("spoken source PCM format invalid"))?;
        require(
            pcm.len() as u64 == count * 6,
            "spoken source decoded duration differs",
        )?;
        sources.insert(key.clone(), bytes);
    }
    check()?;
    fs::create_dir(output).map_err(|_| anyhow::anyhow!("spoken output unavailable or exists"))?;
    let mut owner = OutputOwner {
        root: output.into(),
        names: vec![],
        complete: false,
    };
    let mut editions = Vec::new();
    for (edition_index, edition) in plan.editions.iter().enumerate() {
        check()?;
        let mut mixed = vec![0i64; usize::try_from(edition.duration_samples * 2)?];
        let mut takes = Vec::new();
        for (cue, placement) in edition.recipe.cues.iter().zip(&edition.cues) {
            check()?;
            let source = &sources[&source_key(&cue.performance.source)];
            let pcm = crate::song_edition::wave::pcm24(source)?;
            let processing = operation(policy, &edition.recipe.edition_id, &cue.performance.id);
            let window = PcmWindow {
                source_samples: cue.performance.duration_samples,
                start_sample: cue.selection.start_sample,
                end_sample: cue.selection.end_sample,
                fade_in_samples: processing.fade_in_samples,
                fade_out_samples: processing.fade_out_samples,
            };
            let mut processed = Vec::new();
            let (selected_hash, processed_hash) = pcm_window::transform_with_gain(
                Cursor::new(pcm),
                &mut processed,
                &window,
                processing.gain.numerator,
                processing.gain.denominator,
                &mut check,
            )?;
            for (index, bytes) in processed.chunks_exact(3).enumerate() {
                let target = usize::try_from(placement.start_sample * 2)? + index;
                mixed[target] = mixed[target]
                    .checked_add(pcm_value(bytes))
                    .context("spoken mix integer overflow")?;
            }
            takes.push(TakeEvidence {
                performance_id: cue.performance.id.clone(),
                source: cue.performance.source.clone(),
                selected_pcm_sha256: selected_hash,
                processed_pcm_sha256: processed_hash,
                processing,
            });
        }
        let peak = match profile {
            CompilationProfile::Engineering => 4_194_303,
            CompilationProfile::Spoken => 7_476_354,
        };
        let mut pcm = Vec::with_capacity(mixed.len() * 3);
        for (index, value) in mixed.iter().enumerate() {
            if index % 8192 == 0 {
                check()?;
            }
            require(value.abs() <= peak, "spoken mixed PCM exceeds peak profile")?;
            pcm.extend_from_slice(&(*value as i32).to_le_bytes()[..3]);
        }
        let wave = wave_bytes(&pcm)?;
        require(
            crate::song_edition::wave::pcm24(&wave)? == pcm,
            "spoken candidate structural PCM differs",
        )?;
        let name = format!("edition-{edition_index:03}.wav");
        persist_file(&mut owner, &name, &wave, &mut check)?;
        let processing = takes.iter().map(|t| &t.processing).collect::<Vec<_>>();
        let execution_hash = digest(&serde_json::to_vec(&serde_json::json!({
            "schema":"reel.spoken-execution-recipe.v1","compiled_recipe":edition.recipe_sha256,
            "producer":runtime,"processing":processing}))?);
        editions.push(ProducedEdition {
            edition_id: edition.recipe.edition_id.clone(),
            compiled_recipe_sha256: edition.recipe_sha256.clone(),
            execution_recipe_sha256: execution_hash,
            samples: edition.duration_samples,
            wave: FileRef {
                path: name.into(),
                sha256: digest(&wave),
                bytes: wave.len() as u64,
            },
            pcm_sha256: digest(&pcm),
            takes,
        });
    }
    let receipt = ProductionReceipt {
        schema: RECEIPT_SCHEMA.into(),
        producer: runtime,
        declaration_semantics_sha256: plan.declaration_semantics_sha256,
        compilation_profile: profile,
        editions,
        source_bytes_verified: true,
        producer_structural_checks_passed: true,
        independent_check_passed: false,
        native_admission_verified: false,
        authority_verified: false,
        publication: "not-authorized-by-tool".into(),
    };
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    require(
        bytes.len() <= 16 * 1024 * 1024,
        "spoken receipt exceeds budget",
    )?;
    check()?;
    persist_file(&mut owner, "receipt.json", &bytes, &mut check)?;
    owner.complete = true;
    Ok(receipt)
}

pub(crate) fn bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    File::open(path)
        .map_err(|_| anyhow::anyhow!("spoken control unavailable"))?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("spoken control read failed"))?;
    require(bytes.len() <= limit, "spoken control exceeds budget")?;
    Ok(bytes)
}

pub fn produce_file(
    declaration: &Path,
    plan: &Path,
    processing: Option<&Path>,
    root: &Path,
    output: &Path,
) -> Result<ProductionSummary> {
    let document = load(declaration)?;
    let plan = bounded(plan, 16 * 1024 * 1024)?;
    let policy = processing
        .map(|path| bounded(path, MAX_DECLARATION_BYTES).and_then(|bytes| parse_processing(&bytes)))
        .transpose()?
        .unwrap_or_else(ProcessingPolicy::identity);
    let receipt = produce(&document, &plan, &policy, root, output, || Ok(()))?;
    Ok(ProductionSummary {
        schema: "reel.spoken-production-summary.v1",
        editions: receipt.editions.len(),
        source_bytes_verified: true,
        independent_check_passed: false,
        authority_verified: false,
        publication: "not-authorized-by-tool",
    })
}
