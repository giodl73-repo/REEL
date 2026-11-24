//! Independent lossless signal checking. No producer invocation or promotion.
use super::{
    compiler::{self, CompilationProfile, CompilerIdentity},
    producer::{self, ProcessingPolicy, ProductionControl, ProductionReceipt},
    *,
};
use crate::scene_delivery::InputFiles;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

const MAX_CONTROL: usize = 16 * 1024 * 1024;
pub const ACCESS_SCHEMA: &str = "reel.spoken-access.v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessPackage {
    pub schema: String,
    pub sample_rate: u32,
    pub editions: Vec<AccessEdition>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessEdition {
    pub edition_id: String,
    pub language: String,
    pub compiled_recipe_sha256: String,
    pub execution_recipe_sha256: String,
    pub wave: FileRef,
    pub duration_samples: u64,
    pub role_bindings: Vec<RoleBinding>,
    pub entries: Vec<TranscriptEntry>,
    pub word_evidence: Vec<AccessWordEvidence>,
    pub chapters: Vec<AccessChapter>,
    pub transcript: FileRef,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleBinding {
    pub speaker: u16,
    pub role_sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptEntry {
    pub start_sample: u64,
    pub end_sample: u64,
    pub speaker: Option<u16>,
    pub kind: EntryKind,
    pub text_basis: Option<String>,
    pub text: String,
    pub overlap_before_samples: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind {
    Spoken,
    EngineeringSignal,
    AddedPause,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessChapter {
    pub module_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

/// Recognition evidence remains separate from the selected-script transcript.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessWordEvidence {
    pub speaker: u16,
    pub evidence_text: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Debug, Serialize)]
pub struct CheckSummary {
    pub schema: &'static str,
    pub editions: usize,
    pub independent_pcm_check_passed: bool,
    pub access_checked: bool,
    pub acoustic_word_fidelity_verified: bool,
    pub native_admission_verified: bool,
    pub authority_verified: bool,
    pub publication: &'static str,
}

#[derive(Clone, Copy)]
pub struct CheckRequest<'a> {
    pub document: &'a Document,
    pub plan_bytes: &'a [u8],
    pub receipt_bytes: &'a [u8],
    pub processing: &'a ProcessingPolicy,
    pub expected_runtime: &'a CompilerIdentity,
}

pub struct CheckedPackage {
    plan: compiler::CompiledPlan,
    access: AccessPackage,
    transcripts: Vec<Vec<u8>>,
    summary: CheckSummary,
}
impl CheckedPackage {
    /// Recomputed from the original controls, before independent PCM checking.
    /// Private subjects remain private; this grants no listening/rights authority.
    pub fn compiled_plan(&self) -> &compiler::CompiledPlan {
        &self.plan
    }
    pub fn access(&self) -> &AccessPackage {
        &self.access
    }
    pub fn summary(&self) -> &CheckSummary {
        &self.summary
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn same(a: &impl Serialize, b: &impl Serialize, message: &'static str) -> Result<()> {
    require(
        serde_json::to_value(a)? == serde_json::to_value(b)?,
        message,
    )
}
fn sample(bytes: &[u8]) -> i64 {
    let unsigned = u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16);
    if unsigned & 0x80_0000 != 0 {
        i64::from(unsigned) - 0x100_0000
    } else {
        i64::from(unsigned)
    }
}

/// Sources and candidate are separate trusted byte providers; neither's report
/// is accepted as a signal oracle. The expected runtime comes from the caller's
/// independently bound operation, never from the receipt being checked.
pub fn check(
    request: CheckRequest<'_>,
    sources: &(impl InputFiles + ?Sized),
    candidate: &(impl InputFiles + ?Sized),
    mut control: ProductionControl<impl FnMut() -> Result<()>>,
) -> Result<CheckedPackage> {
    let started = Instant::now();
    require(
        request.plan_bytes.len() <= MAX_CONTROL && request.receipt_bytes.len() <= MAX_CONTROL,
        "spoken check control exceeds budget",
    )?;
    require(
        control.timeout <= Duration::from_secs(30),
        "spoken check deadline exceeds profile",
    )?;
    let deadline = started + control.timeout;
    let mut tick = || -> Result<()> {
        (control.cancellation)()?;
        require(Instant::now() < deadline, "spoken check deadline elapsed")
    };
    tick()?;
    let receipt: ProductionReceipt = serde_json::from_slice(request.receipt_bytes)
        .map_err(|_| anyhow::anyhow!("spoken receipt is not strict JSON"))?;
    require(
        receipt.schema == producer::RECEIPT_SCHEMA
            && receipt.source_bytes_verified
            && receipt.producer_structural_checks_passed
            && !receipt.independent_check_passed
            && !receipt.native_admission_verified
            && !receipt.authority_verified
            && receipt.publication == "not-authorized-by-tool",
        "spoken producer receipt status invalid",
    )?;
    same(
        &receipt.producer,
        request.expected_runtime,
        "spoken producer runtime differs",
    )?;
    let profile = receipt.compilation_profile;
    let plan = compiler::compile_with_inputs(
        request.document,
        sources,
        request.expected_runtime,
        profile,
    )?;
    let raw = serde_json::from_slice(request.plan_bytes)
        .map_err(|_| anyhow::anyhow!("spoken check plan JSON invalid"))?;
    let plan = compiler::replay_selection(plan, &raw)?;
    require(
        serde_json::to_vec_pretty(&plan)? == request.plan_bytes,
        "spoken check plan differs from controls/runtime",
    )?;
    require(
        receipt.declaration_semantics_sha256 == plan.declaration_semantics_sha256
            && receipt.editions.len() == plan.editions.len(),
        "spoken receipt declaration/edition mismatch",
    )?;
    producer::validate_processing(request.processing, &plan)?;
    tick()?;

    let mut pins = BTreeMap::new();
    let mut encoded = 0u64;
    let mut decoded = 0u64;
    let mut output_bytes = 0u64;
    for (index, (edition, produced)) in plan.editions.iter().zip(&receipt.editions).enumerate() {
        require(
            produced.edition_id == edition.recipe.edition_id
                && produced.compiled_recipe_sha256 == edition.recipe_sha256
                && produced.samples == edition.duration_samples
                && produced.takes.len() == edition.recipe.cues.len()
                && produced.wave.path == format!("edition-{index:03}.wav"),
            "spoken output edition/order/shape mismatch",
        )?;
        output_bytes = output_bytes
            .checked_add(produced.wave.bytes)
            .context("spoken output budget overflow")?;
        for cue in &edition.recipe.cues {
            let source = &cue.performance.source;
            let key = (source.path.clone(), source.sha256.clone(), source.bytes);
            if let Some((_, frames)) = pins.get(&key) {
                require(
                    *frames == cue.performance.duration_samples,
                    "spoken source frame declarations disagree",
                )?;
            } else {
                encoded = encoded
                    .checked_add(source.bytes)
                    .context("spoken source budget overflow")?;
                decoded = decoded
                    .checked_add(cue.performance.duration_samples * 6)
                    .context("spoken decoded budget overflow")?;
                pins.insert(key, (source.clone(), cue.performance.duration_samples));
            }
        }
    }
    require(
        encoded <= profile.max_input_pcm_bytes() && decoded <= profile.max_input_pcm_bytes(),
        "spoken check input budget exceeded",
    )?;
    require(
        output_bytes <= profile.max_output_bytes()
            && output_bytes + MAX_CONTROL as u64 <= profile.max_workspace_bytes(),
        "spoken check output budget exceeded",
    )?;
    let mut originals = BTreeMap::new();
    for (key, (pin, frames)) in pins {
        let bytes = producer::read_source(sources, &pin, &mut tick)?;
        let pcm = crate::song_edition::wave::pcm24(&bytes)
            .map_err(|_| anyhow::anyhow!("spoken check source format invalid"))?;
        require(
            pcm.len() as u64 == frames * 6,
            "spoken check source duration differs",
        )?;
        originals.insert(key, bytes);
    }

    let mut access = AccessPackage {
        schema: ACCESS_SCHEMA.into(),
        sample_rate: 48000,
        editions: vec![],
    };
    let mut transcripts = Vec::new();
    let mut text_bytes = 0usize;
    for (index, (edition, produced)) in plan.editions.iter().zip(&receipt.editions).enumerate() {
        tick()?;
        let mut expected = vec![0i64; usize::try_from(edition.duration_samples * 2)?];
        let mut operations = Vec::new();
        for ((cue, placement), reported) in edition
            .recipe
            .cues
            .iter()
            .zip(&edition.cues)
            .zip(&produced.takes)
        {
            let source = &cue.performance.source;
            let original = &originals[&(source.path.clone(), source.sha256.clone(), source.bytes)];
            let pcm = crate::song_edition::wave::pcm24(original)?;
            let start = usize::try_from(cue.selection.start_sample * 6)?;
            let end = usize::try_from(cue.selection.end_sample * 6)?;
            let selected = &pcm[start..end];
            let operation = producer::operation(
                request.processing,
                &edition.recipe.edition_id,
                &cue.performance.id,
            );
            require(
                reported.performance_id == cue.performance.id
                    && reported.selected_pcm_sha256 == digest(selected),
                "spoken selected take evidence differs",
            )?;
            same(
                &reported.source,
                source,
                "spoken selected source reference differs",
            )?;
            same(
                &reported.processing,
                &operation,
                "spoken processing differs from authored policy",
            )?;
            let mut processed = Sha256::new();
            let length = cue.selection.end_sample - cue.selection.start_sample;
            // Independent arithmetic: no PCM-window transform or producer call.
            for (frame, stereo) in selected.chunks_exact(6).enumerate() {
                if frame % 4096 == 0 {
                    tick()?;
                }
                let position = frame as u64;
                for channel in 0..2 {
                    let value = sample(&stereo[channel * 3..channel * 3 + 3]);
                    let gained = i128::from(value) * i128::from(operation.gain.numerator)
                        / i128::from(operation.gain.denominator);
                    let transformed = if position < operation.fade_in_samples {
                        gained * i128::from(position) / i128::from(operation.fade_in_samples)
                    } else if position >= length - operation.fade_out_samples {
                        gained * i128::from(length - position - 1)
                            / i128::from(operation.fade_out_samples)
                    } else {
                        gained
                    };
                    require(
                        (-8_388_608..=8_388_607).contains(&transformed),
                        "spoken checked transform clips",
                    )?;
                    let transformed = transformed as i32;
                    processed.update(&transformed.to_le_bytes()[..3]);
                    let target = usize::try_from(placement.start_sample * 2)? + frame * 2 + channel;
                    expected[target] = expected[target]
                        .checked_add(i64::from(transformed))
                        .context("spoken checked mix overflows")?;
                }
            }
            let processed_hash: String = processed
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            require(
                reported.processed_pcm_sha256 == processed_hash,
                "spoken processed take evidence differs",
            )?;
            operations.push(operation);
        }
        let execution = digest(&serde_json::to_vec(
            &serde_json::json!({"schema":"reel.spoken-execution-recipe.v1",
            "compiled_recipe":edition.recipe_sha256,"producer":request.expected_runtime,"processing":operations}),
        )?);
        require(
            execution == produced.execution_recipe_sha256,
            "spoken execution recipe differs",
        )?;
        let wave = producer::read_source(candidate, &produced.wave, &mut tick)?;
        let actual = crate::song_edition::wave::pcm24(&wave)
            .map_err(|_| anyhow::anyhow!("spoken candidate format invalid"))?;
        require(
            actual.len() == expected.len() * 3 && digest(actual) == produced.pcm_sha256,
            "spoken candidate PCM identity/count differs",
        )?;
        let peak = match profile {
            CompilationProfile::Engineering => 4_194_303,
            CompilationProfile::Spoken => 7_476_354,
        };
        for (i, (bytes, expected)) in actual.chunks_exact(3).zip(expected).enumerate() {
            if i % 8192 == 0 {
                tick()?;
            }
            require(
                expected.abs() <= peak && sample(bytes) == expected,
                "spoken independent PCM comparison failed",
            )?;
        }
        let (text_edition, text) = make_access(index, edition, produced)?;
        text_bytes = text_bytes
            .checked_add(text.len())
            .context("spoken text budget overflow")?;
        require(text_bytes <= MAX_CONTROL, "spoken text budget exceeded")?;
        transcripts.push(text);
        access.editions.push(text_edition);
    }
    require(
        serde_json::to_vec_pretty(&access)?.len() <= MAX_CONTROL,
        "spoken access manifest exceeds budget",
    )?;
    tick()?;
    Ok(CheckedPackage {
        plan,
        summary: CheckSummary {
            schema: "reel.spoken-check.v1",
            editions: access.editions.len(),
            independent_pcm_check_passed: true,
            access_checked: false,
            acoustic_word_fidelity_verified: false,
            native_admission_verified: false,
            authority_verified: false,
            publication: "not-authorized-by-tool",
        },
        access,
        transcripts,
    })
}

fn make_access(
    index: usize,
    edition: &compiler::CompiledEdition,
    produced: &producer::ProducedEdition,
) -> Result<(AccessEdition, Vec<u8>)> {
    let mut roles = BTreeMap::new();
    let mut role_bindings = Vec::new();
    let mut entries = Vec::new();
    let mut previous_end = 0;
    for (cue, placement) in edition.recipe.cues.iter().zip(&edition.cues) {
        if placement.start_sample > previous_end {
            entries.push(TranscriptEntry {
                start_sample: previous_end,
                end_sample: placement.start_sample,
                speaker: None,
                kind: EntryKind::AddedPause,
                text_basis: None,
                text: "[Added pause]".into(),
                overlap_before_samples: 0,
            });
        }
        let next = roles.len() as u16 + 1;
        let speaker = *roles.entry(cue.speaker_id.clone()).or_insert_with(|| {
            role_bindings.push(RoleBinding {
                speaker: next,
                role_sha256: digest(cue.speaker_id.as_bytes()),
            });
            next
        });
        entries.push(TranscriptEntry {
            start_sample: placement.start_sample,
            end_sample: placement.end_sample,
            speaker: Some(speaker),
            kind: if placement.engineering_signal {
                EntryKind::EngineeringSignal
            } else {
                EntryKind::Spoken
            },
            text_basis: if placement.engineering_signal {
                None
            } else {
                Some("selected-script".into())
            },
            text: if placement.engineering_signal {
                "[Engineering signal; no spoken-word evidence]".into()
            } else {
                cue.script.text.clone()
            },
            overlap_before_samples: previous_end.saturating_sub(placement.start_sample),
        });
        previous_end = previous_end.max(placement.end_sample);
    }
    let mut text = String::new();
    for entry in &entries {
        let speaker = entry
            .speaker
            .map(|n| format!("Speaker {n}: "))
            .unwrap_or_default();
        let overlap = if entry.overlap_before_samples > 0 {
            format!(
                " [Intentional overlap: {} samples]",
                entry.overlap_before_samples
            )
        } else {
            String::new()
        };
        text.push_str(&format!(
            "[{}..{} samples] {speaker}{}{overlap}\n",
            entry.start_sample, entry.end_sample, entry.text
        ));
    }
    let text = text.into_bytes();
    Ok((
        AccessEdition {
            edition_id: edition.recipe.edition_id.clone(),
            language: edition.recipe.language.clone(),
            compiled_recipe_sha256: edition.recipe_sha256.clone(),
            execution_recipe_sha256: produced.execution_recipe_sha256.clone(),
            wave: produced.wave.clone(),
            duration_samples: edition.duration_samples,
            role_bindings,
            entries,
            word_evidence: edition
                .word_evidence
                .iter()
                .map(|word| AccessWordEvidence {
                    speaker: roles[&word.speaker_id],
                    evidence_text: word.evidence_text.clone(),
                    start_sample: word.start_sample,
                    end_sample: word.end_sample,
                })
                .collect(),
            chapters: edition
                .chapters
                .iter()
                .map(|c| AccessChapter {
                    module_id: c.module_id.clone(),
                    start_sample: c.start_sample,
                    end_sample: c.end_sample,
                })
                .collect(),
            transcript: FileRef {
                path: format!("edition-{index:03}.txt").into(),
                sha256: digest(&text),
                bytes: text.len() as u64,
            },
        },
        text,
    ))
}

pub fn verify_access(
    checked: &mut CheckedPackage,
    manifest: &[u8],
    files: &(impl InputFiles + ?Sized),
    mut tick: impl FnMut() -> Result<()>,
) -> Result<()> {
    checked.summary.access_checked = false;
    require(
        manifest.len() <= MAX_CONTROL,
        "spoken access manifest exceeds budget",
    )?;
    let access: AccessPackage = serde_json::from_slice(manifest)
        .map_err(|_| anyhow::anyhow!("spoken access is not strict JSON"))?;
    same(
        &access,
        &checked.access,
        "spoken access differs from checked edition",
    )?;
    for (edition, expected) in checked.access.editions.iter().zip(&checked.transcripts) {
        let actual = producer::read_source(files, &edition.transcript, &mut tick)?;
        require(
            actual == *expected,
            "spoken transcript differs from checked source",
        )?;
    }
    tick()?;
    checked.summary.access_checked = true;
    Ok(())
}

pub fn write_access(
    checked: &mut CheckedPackage,
    output: &Path,
    mut tick: impl FnMut() -> Result<()>,
) -> Result<()> {
    checked.summary.access_checked = false;
    tick()?;
    let mut owner = producer::OutputOwner::reserve(output)?;
    for (edition, text) in checked.access.editions.iter().zip(&checked.transcripts) {
        let name = edition
            .transcript
            .path
            .to_str()
            .context("spoken transcript name invalid")?;
        producer::persist_file(&mut owner, name, text, &mut tick)?;
    }
    // Verify the actual written text before publishing the final manifest.
    for (edition, expected) in checked.access.editions.iter().zip(&checked.transcripts) {
        let actual = producer::read_source(output, &edition.transcript, &mut tick)?;
        require(actual == *expected, "spoken written transcript differs")?;
    }
    producer::persist_file(
        &mut owner,
        "access.json",
        &serde_json::to_vec_pretty(&checked.access)?,
        &mut tick,
    )?;
    owner.finish();
    checked.summary.access_checked = true;
    Ok(())
}

pub struct FileCheckRequest<'a> {
    pub declaration: &'a Path,
    pub plan: &'a Path,
    pub processing: Option<&'a Path>,
    pub source_root: &'a Path,
    pub candidate: &'a Path,
    pub access: Option<&'a Path>,
    pub write_access: Option<&'a Path>,
}

pub fn check_file(paths: FileCheckRequest<'_>) -> Result<CheckSummary> {
    Ok(check_file_package(paths, &producer::current_runtime()?)?.summary)
}

/// Check the original controls and return the verified records for local consumers.
pub fn check_file_package(
    paths: FileCheckRequest<'_>,
    runtime: &CompilerIdentity,
) -> Result<CheckedPackage> {
    require(
        paths.access.is_none() || paths.write_access.is_none(),
        "spoken access modes conflict",
    )?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut tick = || require(Instant::now() < deadline, "spoken check deadline elapsed");
    tick()?;
    let document = load(paths.declaration)?;
    let plan = producer::bounded(paths.plan, MAX_CONTROL)?;
    let receipt = producer::bounded(&paths.candidate.join("receipt.json"), MAX_CONTROL)?;
    let processing = paths
        .processing
        .map(|p| {
            producer::bounded(p, MAX_DECLARATION_BYTES)
                .and_then(|bytes| producer::parse_processing(&bytes))
        })
        .transpose()?
        .unwrap_or_else(ProcessingPolicy::identity);
    tick()?;
    let mut checked = check(
        CheckRequest {
            document: &document,
            plan_bytes: &plan,
            receipt_bytes: &receipt,
            processing: &processing,
            expected_runtime: runtime,
        },
        paths.source_root,
        paths.candidate,
        ProductionControl {
            timeout: deadline.saturating_duration_since(Instant::now()),
            cancellation: || Ok(()),
        },
    )?;
    if let Some(root) = paths.access {
        let manifest = producer::bounded(&root.join("access.json"), MAX_CONTROL)?;
        verify_access(&mut checked, &manifest, root, &mut tick)?;
        tick()?;
    }
    if let Some(root) = paths.write_access {
        write_access(&mut checked, root, &mut tick)?;
    }
    Ok(checked)
}
