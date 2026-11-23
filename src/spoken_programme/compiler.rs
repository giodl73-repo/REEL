//! Control-metadata compilation only. No media, authority or native admission proof.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result};
use reel_assembly::scene_authoring::{WORD_TIMING_SCHEMA, WordTimingEvidence, verify_word_text};
use serde::{Deserialize, Serialize};

use super::*;
use crate::scene_delivery::InputFiles;

pub const ALIGNMENT_SCHEMA: &str = "reel.spoken-alignment.v1";
pub const PLAN_SCHEMA: &str = "reel.spoken-edition-plan.v1";
const MAX_METADATA: u64 = 4 * 1024 * 1024;
const MAX_PLAN: usize = 16 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum CompilationProfile {
    Engineering,
    Spoken,
}

impl CompilationProfile {
    pub fn max_samples(self) -> u64 {
        match self {
            Self::Engineering => 720_000,
            Self::Spoken => 1_440_000,
        }
    }

    pub fn max_input_pcm_bytes(self) -> u64 {
        match self {
            Self::Engineering => 16 * 1024 * 1024,
            Self::Spoken => 32 * 1024 * 1024,
        }
    }

    pub fn max_output_bytes(self) -> u64 {
        64 * 1024 * 1024
    }
    pub fn max_workspace_bytes(self) -> u64 {
        128 * 1024 * 1024
    }
    pub fn timeout_seconds(self) -> u64 {
        30
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerIdentity {
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AlignmentEvidence {
    pub schema: String,
    pub performance_id: String,
    pub cue_id: String,
    pub language: String,
    pub source: FileRef,
    pub sample_rate: u32,
    pub duration_samples: u64,
    pub script_id: String,
    pub script_sha256: String,
    pub observation: Observation,
    pub markers: Vec<CueMarker>,
    pub word_bindings: Vec<WordBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Observation {
    EngineeringSignal {},
    WordEvidence {
        observed_text: String,
        timing: WordTimingEvidence,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WordBinding {
    pub marker_id: String,
    pub first_word_index: usize,
    pub end_word_index_exclusive: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConsumedCue {
    pub module_id: String,
    pub required_modules: Vec<String>,
    pub cue_id: String,
    pub speaker_id: String,
    pub script: Script,
    pub performance: Performance,
    pub selection: TakeUse,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConsumedRecipe {
    pub schema: &'static str,
    pub compiler: CompilerIdentity,
    pub document_id: String,
    pub edition_id: String,
    pub language: String,
    pub audience: String,
    pub required_modules: Vec<String>,
    pub sample_rate: u32,
    pub output_profile: &'static str,
    pub compilation_profile: CompilationProfile,
    pub cues: Vec<ConsumedCue>,
    pub joins: Vec<Join>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CuePlacement {
    pub module_id: String,
    pub cue_id: String,
    pub performance_id: String,
    pub speaker_id: String,
    pub source_start_sample: u64,
    pub source_end_sample: u64,
    pub start_sample: u64,
    pub end_sample: u64,
    pub engineering_signal: bool,
    pub observed_text: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Chapter {
    pub module_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlacedWordEvidence {
    pub performance_id: String,
    pub speaker_id: String,
    pub evidence_text: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlacedMarker {
    pub performance_id: String,
    pub marker: CueMarker,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompiledEdition {
    pub recipe_sha256: String,
    pub recipe: ConsumedRecipe,
    pub duration_samples: u64,
    pub cues: Vec<CuePlacement>,
    pub chapters: Vec<Chapter>,
    pub word_evidence: Vec<PlacedWordEvidence>,
    pub markers: Vec<PlacedMarker>,
}

#[derive(Debug, Serialize)]
pub struct CompiledPlan {
    pub schema: &'static str,
    /// Canonical typed declaration identity; not an edition consumption hash.
    pub declaration_semantics_sha256: String,
    pub editions: Vec<CompiledEdition>,
    pub alignment_metadata_verified: bool,
    pub media_verified: bool,
    pub authority_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct CompilationSummary {
    pub schema: &'static str,
    pub editions: usize,
    pub cues: usize,
    pub alignment_metadata_verified: bool,
    pub media_verified: bool,
    pub authority_verified: bool,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn canonical_hash(value: &impl Serialize) -> Result<String> {
    Ok(digest(&serde_json::to_vec(value)?))
}

fn spend_derived(budget: &mut usize, value: &impl Serialize) -> Result<()> {
    *budget = budget
        .checked_add(serde_json::to_vec(value)?.len())
        .context("spoken derived metadata budget overflow")?;
    require(
        *budget <= 8 * 1024 * 1024,
        "spoken derived metadata exceeds budget",
    )
}

fn at<T>(result: Result<T>, kind: &'static str, index: usize) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("spoken {kind} index {index}: {error}"))
}

fn selected_ids(document: &Document) -> BTreeSet<String> {
    document
        .editions
        .iter()
        .flat_map(|e| &e.selections)
        .flat_map(|s| &s.takes)
        .map(|t| t.performance_id.clone())
        .collect()
}

fn same_pin(a: &FileRef, b: &FileRef) -> bool {
    a.path == b.path && a.sha256 == b.sha256 && a.bytes == b.bytes
}

fn same_marker(a: &CueMarker, b: &CueMarker) -> bool {
    a.id == b.id
        && a.start_sample == b.start_sample
        && a.end_sample == b.end_sample
        && std::mem::discriminant(&a.kind) == std::mem::discriminant(&b.kind)
}

fn check_alignment(p: &Performance, script: &Script, evidence: &AlignmentEvidence) -> Result<()> {
    require(
        evidence.schema == ALIGNMENT_SCHEMA
            && evidence.performance_id == p.id
            && evidence.cue_id == p.cue_id
            && evidence.language == p.language
            && same_pin(&evidence.source, &p.source)
            && evidence.sample_rate == 48_000
            && evidence.duration_samples == p.duration_samples
            && evidence.script_id == p.script_id
            && evidence.script_sha256 == p.script_sha256,
        "spoken alignment identity mismatch",
    )?;
    require(
        evidence.markers.len() == p.markers.len()
            && evidence
                .markers
                .iter()
                .zip(&p.markers)
                .all(|(a, b)| same_marker(a, b)),
        "spoken alignment marker mismatch",
    )?;
    match (&p.scope.kind, &evidence.observation) {
        (PerformanceKind::EngineeringSignal, Observation::EngineeringSignal {}) => {
            require(
                p.observed_text_sha256 == digest(b"") && evidence.word_bindings.is_empty(),
                "spoken engineering observation invalid",
            )?;
        }
        (
            PerformanceKind::RecordedSpeech,
            Observation::WordEvidence {
                observed_text,
                timing,
            },
        ) => {
            require(
                observed_text.len() <= 8192
                    && digest(observed_text.as_bytes()) == p.observed_text_sha256
                    && timing.schema == WORD_TIMING_SCHEMA
                    && timing.language == p.language
                    && timing.cue_id == p.cue_id
                    && timing.selected_take_sha256 == p.source.sha256
                    && timing.sample_rate == 48_000
                    && timing.cue_end_sample == p.duration_samples
                    && !timing.words.is_empty()
                    && timing.words.len() <= 1024,
                "spoken word evidence identity mismatch",
            )?;
            let mut previous = 0;
            for word in &timing.words {
                require(
                    word.start_sample >= previous
                        && word.start_sample < word.end_sample
                        && word.end_sample <= p.duration_samples
                        && word.word.len() <= 8192
                        && !word.word.chars().any(char::is_control),
                    "spoken word clock or text invalid",
                )?;
                previous = word.start_sample;
            }
            verify_word_text(timing, observed_text)
                .map_err(|_| anyhow::anyhow!("spoken observed word sequence differs"))?;
            verify_word_text(timing, &script.text)
                .map_err(|_| anyhow::anyhow!("spoken script word sequence differs"))?;
            require(
                evidence.word_bindings.len() == p.markers.len()
                    && unique(evidence.word_bindings.iter().map(|b| b.marker_id.as_str())),
                "spoken word binding count or identity invalid",
            )?;
            for marker in &p.markers {
                let binding = evidence
                    .word_bindings
                    .iter()
                    .find(|b| b.marker_id == marker.id)
                    .context("spoken marker word binding missing")?;
                require(
                    binding.first_word_index < binding.end_word_index_exclusive
                        && binding.end_word_index_exclusive <= timing.words.len(),
                    "spoken word binding bounds invalid",
                )?;
                let words =
                    &timing.words[binding.first_word_index..binding.end_word_index_exclusive];
                require(
                    (!matches!(marker.kind, crate::cue_relative::MarkerKind::Word)
                        || words.len() == 1)
                        && marker.start_sample == words[0].start_sample
                        && marker.end_sample
                            == words.iter().map(|w| w.end_sample).max().unwrap_or(0),
                    "spoken marker differs from native words",
                )?;
            }
        }
        _ => require(false, "spoken observation kind differs from declaration")?,
    }
    Ok(())
}

/// Verify exact selected control bytes, then compile. This function has no IO ports.
pub fn compile(
    document: &Document,
    metadata: &BTreeMap<String, Vec<u8>>,
    compiler: &CompilerIdentity,
    profile: CompilationProfile,
) -> Result<CompiledPlan> {
    validate(document)?;
    let declaration_bytes = serde_json::to_vec(document)?;
    require(
        declaration_bytes.len() <= MAX_DECLARATION_BYTES,
        "spoken declaration exceeds budget",
    )?;
    require(
        hash(&compiler.sha256) && compiler.bytes > 0,
        "spoken compiler identity invalid",
    )?;
    let selected = selected_ids(document);
    require(
        metadata.keys().cloned().collect::<BTreeSet<_>>() == selected,
        "spoken selected evidence set mismatch",
    )?;
    let mut total_metadata = 0u64;
    let mut evidence = BTreeMap::new();
    for (performance_index, p) in document
        .performances
        .iter()
        .enumerate()
        .filter(|(_, p)| selected.contains(&p.id))
    {
        let bytes = &metadata[&p.id];
        total_metadata = total_metadata
            .checked_add(bytes.len() as u64)
            .context("spoken metadata budget overflow")?;
        require(
            bytes.len() <= MAX_DECLARATION_BYTES && total_metadata <= MAX_METADATA,
            "spoken alignment metadata exceeds budget",
        )?;
        require(
            bytes.len() as u64 == p.alignment.bytes && digest(bytes) == p.alignment.sha256,
            "spoken alignment bytes differ from pin",
        )?;
        let item: AlignmentEvidence = at(
            serde_json::from_slice(bytes)
                .map_err(|_| anyhow::anyhow!("spoken alignment is not strict JSON")),
            "performance",
            performance_index,
        )?;
        let script = document
            .modules
            .iter()
            .find(|m| m.id == p.module_id)
            .and_then(|m| m.cues.iter().find(|c| c.id == p.cue_id))
            .and_then(|c| c.scripts.iter().find(|s| s.id == p.script_id))
            .context("spoken selected script missing")?;
        at(
            check_alignment(p, script, &item),
            "performance",
            performance_index,
        )?;
        evidence.insert(p.id.clone(), item);
    }
    let mut editions = Vec::new();
    let mut derived_bytes = 0usize;
    for (edition_index, edition) in document.editions.iter().enumerate() {
        let compiled = (|| -> Result<CompiledEdition> {
            let mut cues = Vec::new();
            let mut chapters = Vec::new();
            let mut words = Vec::new();
            let mut markers = Vec::new();
            let mut consumed = Vec::new();
            let mut cursor = 0u64;
            for selection in &edition.selections {
                let module = document
                    .modules
                    .iter()
                    .find(|m| m.id == selection.module_id)
                    .context("spoken selected module missing")?;
                let mut chapter_start = None;
                for (take, cue) in selection.takes.iter().zip(&module.cues) {
                    let p = document
                        .performances
                        .iter()
                        .find(|p| p.id == take.performance_id)
                        .context("spoken selected performance missing")?;
                    if !cues.is_empty() {
                        let join = &edition.joins[cues.len() - 1];
                        cursor = cursor
                            .checked_add(join.added_pause_samples)
                            .and_then(|n| n.checked_sub(join.overlap_samples))
                            .context("spoken placement arithmetic overflow")?;
                    }
                    let start = cursor;
                    cursor = start
                        .checked_add(take.end_sample - take.start_sample)
                        .context("spoken placement arithmetic overflow")?;
                    require(
                        cursor <= profile.max_samples(),
                        "spoken edition duration exceeds profile",
                    )?;
                    chapter_start.get_or_insert(start);
                    let shift = |a: u64, b: u64| -> Result<(u64, u64)> {
                        require(
                            a >= take.start_sample && b <= take.end_sample && a < b,
                            "spoken selected trim excludes word or marker",
                        )?;
                        Ok((start + a - take.start_sample, start + b - take.start_sample))
                    };
                    let alignment = &evidence[&p.id];
                    if let Observation::WordEvidence { timing, .. } = &alignment.observation {
                        for word in &timing.words {
                            let (a, b) =
                                at(shift(word.start_sample, word.end_sample), "cue", cues.len())?;
                            let placed = PlacedWordEvidence {
                                performance_id: p.id.clone(),
                                speaker_id: cue.speaker_id.clone(),
                                evidence_text: word.word.clone(),
                                start_sample: a,
                                end_sample: b,
                            };
                            spend_derived(&mut derived_bytes, &placed)?;
                            words.push(placed);
                        }
                    }
                    for marker in &p.markers {
                        let (a, b) = at(
                            shift(marker.start_sample, marker.end_sample),
                            "cue",
                            cues.len(),
                        )?;
                        let mut placed = marker.clone();
                        placed.start_sample = a;
                        placed.end_sample = b;
                        let placed = PlacedMarker {
                            performance_id: p.id.clone(),
                            marker: placed,
                        };
                        spend_derived(&mut derived_bytes, &placed)?;
                        markers.push(placed);
                    }
                    let placed = CuePlacement {
                        module_id: module.id.clone(),
                        cue_id: cue.id.clone(),
                        performance_id: p.id.clone(),
                        speaker_id: cue.speaker_id.clone(),
                        source_start_sample: take.start_sample,
                        source_end_sample: take.end_sample,
                        start_sample: start,
                        end_sample: cursor,
                        engineering_signal: matches!(
                            p.scope.kind,
                            PerformanceKind::EngineeringSignal
                        ),
                        observed_text: match &alignment.observation {
                            Observation::EngineeringSignal {} => None,
                            Observation::WordEvidence { observed_text, .. } => {
                                Some(observed_text.clone())
                            }
                        },
                    };
                    spend_derived(&mut derived_bytes, &placed)?;
                    cues.push(placed);
                    let mut requirements = module.requires.clone();
                    requirements.sort();
                    // Consume this edition's authorization predicate, not other consumers' IDs.
                    let mut performance = p.clone();
                    performance.scope.edition_ids = vec![edition.id.clone()];
                    let script = cue
                        .scripts
                        .iter()
                        .find(|s| s.id == p.script_id)
                        .context("spoken selected script missing")?;
                    consumed.push(ConsumedCue {
                        module_id: module.id.clone(),
                        required_modules: requirements,
                        cue_id: cue.id.clone(),
                        speaker_id: cue.speaker_id.clone(),
                        script: script.clone(),
                        performance,
                        selection: take.clone(),
                    });
                }
                chapters.push(Chapter {
                    module_id: module.id.clone(),
                    start_sample: chapter_start.context("spoken empty chapter")?,
                    end_sample: cursor,
                });
            }
            let mut requirements = edition.required_modules.clone();
            requirements.sort();
            let recipe = ConsumedRecipe {
                schema: "reel.spoken-consumption.v1",
                compiler: compiler.clone(),
                document_id: document.id.clone(),
                edition_id: edition.id.clone(),
                language: edition.language.clone(),
                audience: edition.audience.clone(),
                required_modules: requirements,
                sample_rate: document.sample_rate,
                output_profile: "wav-pcm24-stereo-48000",
                compilation_profile: profile,
                cues: consumed,
                joins: edition.joins.clone(),
            };
            Ok(CompiledEdition {
                recipe_sha256: canonical_hash(&recipe)?,
                recipe,
                duration_samples: cursor,
                cues,
                chapters,
                word_evidence: words,
                markers,
            })
        })();
        editions.push(at(compiled, "edition", edition_index)?);
    }
    let plan = CompiledPlan {
        schema: PLAN_SCHEMA,
        declaration_semantics_sha256: digest(&declaration_bytes),
        editions,
        alignment_metadata_verified: true,
        media_verified: false,
        authority_verified: false,
    };
    require(
        serde_json::to_vec_pretty(&plan)?.len() <= MAX_PLAN,
        "spoken compiled plan exceeds budget",
    )?;
    Ok(plan)
}

/// Read only named alignment metadata; unrelated provider objects are never enumerated.
pub fn compile_with_inputs(
    document: &Document,
    inputs: &(impl InputFiles + ?Sized),
    compiler: &CompilerIdentity,
    profile: CompilationProfile,
) -> Result<CompiledPlan> {
    validate(document)?;
    let selected = selected_ids(document);
    let mut declared_bytes = 0u64;
    for p in document
        .performances
        .iter()
        .filter(|p| selected.contains(&p.id))
    {
        declared_bytes = declared_bytes
            .checked_add(p.alignment.bytes)
            .context("spoken metadata budget overflow")?;
        require(
            p.alignment.bytes <= MAX_DECLARATION_BYTES as u64 && declared_bytes <= MAX_METADATA,
            "spoken alignment metadata exceeds budget",
        )?;
    }
    let mut metadata = BTreeMap::new();
    for p in document
        .performances
        .iter()
        .filter(|p| selected.contains(&p.id))
    {
        let bytes = inputs
            .control_bytes(&p.alignment)
            .map_err(|_| anyhow::anyhow!("spoken alignment metadata unavailable"))?;
        require(
            bytes.len() <= MAX_DECLARATION_BYTES,
            "spoken alignment metadata exceeds budget",
        )?;
        metadata.insert(p.id.clone(), bytes);
    }
    compile(document, &metadata, compiler, profile)
}

fn current_compiler() -> Result<CompilerIdentity> {
    let executable = std::env::current_exe()
        .map_err(|_| anyhow::anyhow!("spoken compiler executable unavailable"))?;
    Ok(CompilerIdentity {
        sha256: crate::sha256_file(&executable)
            .map_err(|_| anyhow::anyhow!("spoken compiler identity unavailable"))?,
        bytes: std::fs::metadata(&executable)
            .map_err(|_| anyhow::anyhow!("spoken compiler size unavailable"))?
            .len(),
    })
}

fn compile_local(
    declaration: &Path,
    document: &Document,
    compiler: &CompilerIdentity,
    profile: CompilationProfile,
) -> Result<CompiledPlan> {
    let root = declaration
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    compile_with_inputs(document, root, compiler, profile)
}

#[derive(Debug, Serialize)]
pub struct EditionImpact {
    pub edition_id: String,
    pub language: String,
    pub action: &'static str,
    pub previous_recipe_sha256: Option<String>,
    pub current_recipe_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImpactReport {
    pub schema: &'static str,
    pub document_id: String,
    pub previous_declaration_sha256: String,
    pub current_declaration_sha256: String,
    pub editions: Vec<EditionImpact>,
    pub reuse_requires_checked_retained_output: bool,
}

pub fn impact_file(
    previous: &Path,
    current: &Path,
    profile: CompilationProfile,
) -> Result<ImpactReport> {
    let previous_document = load(previous)?;
    let current_document = load(current)?;
    require(
        previous_document.id == current_document.id,
        "spoken impact declarations must name the same document",
    )?;
    let compiler = current_compiler()?;
    let before = compile_local(previous, &previous_document, &compiler, profile)?;
    let after = compile_local(current, &current_document, &compiler, profile)?;
    Ok(edition_impact(current_document.id, before, after))
}

fn edition_impact(document_id: String, before: CompiledPlan, after: CompiledPlan) -> ImpactReport {
    let mut editions = BTreeMap::new();
    for edition in before.editions {
        let id = edition.recipe.edition_id;
        editions.insert(
            id.clone(),
            EditionImpact {
                edition_id: id,
                language: edition.recipe.language,
                action: "removed",
                previous_recipe_sha256: Some(edition.recipe_sha256),
                current_recipe_sha256: None,
            },
        );
    }
    for edition in after.editions {
        let id = edition.recipe.edition_id;
        let impact = editions.entry(id.clone()).or_insert(EditionImpact {
            edition_id: id,
            language: edition.recipe.language.clone(),
            action: "build",
            previous_recipe_sha256: None,
            current_recipe_sha256: None,
        });
        impact.language = edition.recipe.language;
        impact.current_recipe_sha256 = Some(edition.recipe_sha256);
        if impact.previous_recipe_sha256.is_some() {
            impact.action = if impact.previous_recipe_sha256 == impact.current_recipe_sha256 {
                "reuse"
            } else {
                "rebuild"
            };
        }
    }
    ImpactReport {
        schema: "reel.spoken-edition-impact.v1",
        document_id,
        previous_declaration_sha256: before.declaration_semantics_sha256,
        current_declaration_sha256: after.declaration_semantics_sha256,
        editions: editions.into_values().collect(),
        reuse_requires_checked_retained_output: true,
    }
}

pub fn compile_file(
    declaration: &Path,
    output: &Path,
    profile: CompilationProfile,
) -> Result<CompilationSummary> {
    let document = load(declaration)?;
    let plan = compile_local(declaration, &document, &current_compiler()?, profile)?;
    let bytes = serde_json::to_vec_pretty(&plan)?;
    require(
        bytes.len() <= MAX_PLAN,
        "spoken compiled plan exceeds budget",
    )?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| anyhow::anyhow!("spoken plan staging unavailable"))?;
    staged
        .write_all(&bytes)
        .map_err(|_| anyhow::anyhow!("spoken plan staging write failed"))?;
    staged
        .persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("spoken plan destination unavailable or exists"))?;
    Ok(CompilationSummary {
        schema: "reel.spoken-edition-compilation.v1",
        editions: plan.editions.len(),
        cues: plan.editions.iter().map(|e| e.cues.len()).sum(),
        alignment_metadata_verified: true,
        media_verified: false,
        authority_verified: false,
    })
}
