//! A presentation-only successor of an exact, historically selected scene.
//! This route never permits edits to the underlying picture or sound selection.

use crate::{imported_scene_proof, scene_delivery};
use anyhow::{Context, Result, bail};
use reel_assembly::{
    scene_authoring::NativeAlignment,
    template_presentation::{
        EditableTextInvocation, EditableTextTemplate, PresentationSourceText, compile_layer,
        verify_source_text,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path};

pub const MANIFEST_SCHEMA: &str = "reel.imported-scene-presentation-successor-manifest.v1";
pub const DESCRIPTOR_SCHEMA: &str = "reel.imported-scene-presentation-successor.v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub schema: String,
    pub original_manifest: scene_delivery::FileRef,
    pub original_receipt: scene_delivery::FileRef,
    pub original_cached_semantic: scene_delivery::FileRef,
    pub template_definition: scene_delivery::FileRef,
    pub template_catalog: scene_delivery::FileRef,
    pub presentation_bindings: Vec<scene_delivery::FileRef>,
    pub source_text_binding: String,
    pub source_text: scene_delivery::FileRef,
    pub compile_receipt: scene_delivery::FileRef,
    pub ass: scene_delivery::FileRef,
    pub font: scene_delivery::FileRef,
    pub attachment_id: String,
    /// Identity used by the separately authored presentation projection.
    pub presentation_scene_id: String,
    pub source_authority_id: String,
    pub source_scope_ids: Vec<String>,
    pub invocation: EditableTextInvocation,
    /// Ordered source cue aliases mapped to the original native cue clocks.
    /// Each alias must be witnessed by the original narration logical ID.
    pub cue_bindings: Vec<CueBinding>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CueBinding {
    pub source_cue_id: String,
    pub native_cue_id: String,
    pub narration_logical_id: String,
}

pub struct VerifiedOverlay {
    pub original: Box<imported_scene_proof::VerifiedInputs>,
    pub descriptor: Descriptor,
    pub descriptor_sha256: String,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_yaml::from_slice(&fs::read(path)?).with_context(|| path.display().to_string())
}

/// Compare raw documents so unknown controls and default-valued changes cannot
/// disappear through typed deserialization. Exactly one layer is appended.
fn additive_documents(
    original_job: &Value,
    derived_job: &Value,
    original_contract: &Value,
    derived_contract: &Value,
    descriptor: &Descriptor,
) -> Result<()> {
    let mut before = original_job
        .as_object()
        .context("original job is not an object")?
        .clone();
    let mut after = derived_job
        .as_object()
        .context("derived job is not an object")?
        .clone();
    let old_encoding = before.remove("still_sequence_encoding");
    let new_encoding = after.remove("still_sequence_encoding");
    if old_encoding != new_encoding
        && !(old_encoding.as_ref().is_none_or(Value::is_null)
            && new_encoding == Some(serde_json::json!("h264-lossless")))
    {
        bail!("presentation successor changes unsupported encoding");
    }
    before.remove("contract");
    after.remove("contract");
    let original_layers = before
        .remove("external_layers")
        .unwrap_or(serde_json::json!([]));
    let mut layers = after
        .remove("external_layers")
        .context("presentation layer missing")?
        .as_array()
        .context("invalid external layers")?
        .clone();
    let added = layers.pop().context("presentation layer missing")?;
    if Value::Array(layers) != original_layers || before != after {
        bail!("presentation successor changes original picture, audio or job controls");
    }
    let layer: scene_delivery::ExternalLayer = serde_json::from_value(added)?;
    if layer.attachment_id != descriptor.attachment_id
        || layer.evidence != descriptor.ass
        || layer.font.as_ref() != Some(&descriptor.font)
        || serde_json::to_value(&layer.render_mode)? != serde_json::json!("ass-overlay")
        || layer.render_source.is_some()
        || layer.derivation_receipt.is_some()
    {
        bail!("presentation successor layer differs from selected ASS/font");
    }
    let mut before = original_contract
        .as_object()
        .context("invalid original contract")?
        .clone();
    let mut after = derived_contract
        .as_object()
        .context("invalid derived contract")?
        .clone();
    let originals = before
        .remove("attachments")
        .unwrap_or(serde_json::json!([]));
    let mut attachments = after
        .remove("attachments")
        .context("title attachment missing")?
        .as_array()
        .context("invalid attachments")?
        .clone();
    let mut added = attachments.pop().context("title attachment missing")?;
    // Explicit zero offsets are equivalent to omitted offsets; no other raw
    // attachment control is normalized away.
    for edge in ["start", "end"] {
        if added[edge].get("offset_samples") == Some(&serde_json::json!(0)) {
            added[edge]
                .as_object_mut()
                .context("invalid title anchor")?
                .remove("offset_samples");
        }
    }
    let contract: crate::cue_relative::Contract =
        serde_json::from_value(original_contract.clone())?;
    let first = contract
        .cues
        .first()
        .context("original scene has no cues")?;
    let last = contract.cues.last().context("original scene has no cues")?;
    let expected = serde_json::json!({"id":descriptor.attachment_id,
        "target":{"kind":"title","title_id":descriptor.attachment_id},
        "start":{"kind":"cue-start","cue_id":first.cue_id},
        "end":{"kind":"cue-end","cue_id":last.cue_id}});
    if Value::Array(attachments) != originals || before != after || added != expected {
        bail!("presentation successor must append only one full-scene title attachment");
    }
    Ok(())
}

pub(crate) fn verify_inputs(
    root: &Path,
    manifest_ref: &scene_delivery::FileRef,
    manifest: imported_scene_proof::Manifest,
) -> Result<imported_scene_proof::VerifiedInputs> {
    if manifest.schema != MANIFEST_SCHEMA || manifest.encoding_successor.is_some() {
        bail!("presentation successor requires its own exclusive descriptor");
    }
    let descriptor_ref = manifest
        .presentation_successor
        .as_ref()
        .context("presentation descriptor missing")?;
    let descriptor: Descriptor = read(&scene_delivery::checked_file(root, descriptor_ref)?)?;
    if descriptor.schema != DESCRIPTOR_SCHEMA || descriptor.attachment_id.trim().is_empty() {
        bail!("invalid presentation successor descriptor");
    }
    let original =
        imported_scene_proof::verify_inputs_inner(root, &descriptor.original_manifest, false)?;
    let old = &original.manifest;
    if old.schema != imported_scene_proof::MANIFEST_SCHEMA
        || original.encoding.is_some()
        || original.overlay.is_some()
    {
        bail!("presentation successor must reference one unchanged original import");
    }
    for evidence in [
        &descriptor.original_receipt,
        &descriptor.original_cached_semantic,
    ] {
        if !old.source_evidence.contains(evidence) {
            bail!("presentation historical evidence is not retained in original import");
        }
    }
    if manifest.episode_id != old.episode_id
        || manifest.scene_id != old.scene_id
        || manifest.language != old.language
        || manifest.source_target != old.source_target
        || manifest.presentation_role != old.presentation_role
        || manifest.source_graph != old.source_graph
        || manifest.source_selection != old.source_selection
        || manifest.source_capture != old.source_capture
        || manifest.semantic_delivery != old.semantic_delivery
        || manifest.production != old.production
    {
        bail!("presentation successor changes original identity or source evidence");
    }
    let job_path = scene_delivery::checked_file(root, &manifest.job)?;
    let original_job: scene_delivery::Job = read(&original.job_path)?;
    let derived_job: scene_delivery::Job = read(&job_path)?;
    let old_contract = scene_delivery::checked_file(
        original.job_path.parent().context("original job parent")?,
        &original_job.contract,
    )?;
    let new_contract = scene_delivery::checked_file(
        job_path.parent().context("derived job parent")?,
        &derived_job.contract,
    )?;
    additive_documents(
        &read(&original.job_path)?,
        &read(&job_path)?,
        &read(&old_contract)?,
        &read(&new_contract)?,
        &descriptor,
    )?;
    for evidence in &manifest.source_evidence {
        scene_delivery::checked_file(root, evidence)?;
    }
    for evidence in [
        &descriptor.original_receipt,
        &descriptor.original_cached_semantic,
        &descriptor.template_definition,
        &descriptor.template_catalog,
        &descriptor.source_text,
        &descriptor.compile_receipt,
    ] {
        scene_delivery::checked_file(root, evidence)?;
    }
    if descriptor.presentation_bindings.is_empty() {
        bail!("presentation successor lacks selected scoped bindings");
    }
    for evidence in &descriptor.presentation_bindings {
        scene_delivery::checked_file(root, evidence)?;
    }
    let original_semantic: crate::semantic_delivery::SemanticDelivery =
        read(&scene_delivery::checked_file(root, &old.semantic_delivery)?)?;
    let cached: crate::semantic_delivery::SemanticDelivery = read(&scene_delivery::checked_file(
        root,
        &descriptor.original_cached_semantic,
    )?)?;
    if cached.schema != "reel.semantic-delivery.v1"
        || cached.target != old.source_target
        || cached.language != original_semantic.language
        || cached
            .scene_id
            .as_deref()
            .is_some_and(|id| id != old.scene_id)
        || cached.scene_delivery_job.sha256 != old.job.sha256
        || serde_json::to_value(&cached.event_bindings)?
            != serde_json::to_value(&original_semantic.event_bindings)?
        || super::imported_scene_proof::scene_projection(&cached.graph, &old.scene_id)?
            != super::imported_scene_proof::scene_projection(
                &original_semantic.graph,
                &old.scene_id,
            )?
        || serde_json::to_value(&cached.pointer.selected_lock)?
            != serde_json::to_value(&cached.graph.lock)?
    {
        bail!("presentation original cached semantic differs from selected source");
    }
    let contract_sha256 = derived_job.contract.sha256;
    let descriptor_sha256 = descriptor_ref.sha256.clone();
    Ok(imported_scene_proof::VerifiedInputs {
        input_root: root.to_path_buf(),
        manifest,
        job_path,
        contract_sha256,
        manifest_sha256: manifest_ref.sha256.clone(),
        encoding: None,
        overlay: Some(VerifiedOverlay {
            original: Box::new(original),
            descriptor,
            descriptor_sha256,
        }),
    })
}

pub(crate) fn verify_native(
    inputs: &imported_scene_proof::VerifiedInputs,
    assets: &Path,
) -> Result<()> {
    let overlay = inputs
        .overlay
        .as_ref()
        .context("presentation successor missing")?;
    let d = &overlay.descriptor;
    let root = &inputs.input_root;
    let (original_job, original_plan) = scene_delivery::plan(&overlay.original.job_path, assets)?;
    let historical: scene_delivery::Receipt =
        read(&scene_delivery::checked_file(root, &d.original_receipt)?)?;
    if historical.schema != "reel.scene-delivery-receipt.v0.1"
        || serde_json::to_value(&historical.plan)? != serde_json::to_value(&original_plan)?
    {
        bail!("presentation original receipt has a stale compiled plan");
    }
    let (_, derived_plan) = scene_delivery::plan(&inputs.job_path, assets)?;
    let mut before = serde_json::to_value(&original_plan)?;
    let mut after = serde_json::to_value(&derived_plan)?;
    for key in [
        "job_sha256",
        "contract_sha256",
        "compiled_sha256",
        "external_layer_spans",
        "rendered_external_layers",
    ] {
        before.as_object_mut().unwrap().remove(key);
        after.as_object_mut().unwrap().remove(key);
    }
    let mut expected_layers = original_plan.external_layers.clone();
    expected_layers.push(d.attachment_id.clone());
    if derived_plan.external_layers != expected_layers {
        bail!("presentation successor has unexpected external layer identities");
    }
    before.as_object_mut().unwrap().remove("external_layers");
    after.as_object_mut().unwrap().remove("external_layers");
    if before != after
        || derived_plan.external_layer_spans.len() != original_plan.external_layer_spans.len() + 1
        || serde_json::to_value(
            &derived_plan.external_layer_spans[..original_plan.external_layer_spans.len()],
        )? != serde_json::to_value(&original_plan.external_layer_spans)?
    {
        bail!("presentation successor changes original compiled content or clocks");
    }
    let added = derived_plan
        .external_layer_spans
        .last()
        .context("overlay span missing")?;
    if added.attachment_id != d.attachment_id
        || added.start_sample != 0
        || added.end_sample != original_plan.duration_samples
    {
        bail!("presentation layer does not span original scene");
    }
    let template: EditableTextTemplate =
        read(&scene_delivery::checked_file(root, &d.template_definition)?)?;
    let source: PresentationSourceText =
        read(&scene_delivery::checked_file(root, &d.source_text)?)?;
    let catalog: reel_assembly::scene_authoring::TemplateCatalog =
        read(&scene_delivery::checked_file(root, &d.template_catalog)?)?;
    if catalog.schema != reel_assembly::scene_authoring::CATALOG_SCHEMA
        || catalog
            .templates
            .iter()
            .filter(|entry| {
                entry.template_id == template.template_id
                    && entry.kind == template.kind
                    && entry.definition_sha256 == d.template_definition.sha256
            })
            .count()
            != 1
    {
        bail!("presentation template differs from selected catalog definition");
    }
    let mut selections = Vec::new();
    for reference in &d.presentation_bindings {
        let scope: reel_assembly::scene_authoring::ScopedBindings =
            read(&scene_delivery::checked_file(root, reference)?)?;
        if scope.schema != reel_assembly::scene_authoring::BINDINGS_SCHEMA
            || scope.scope_id.trim().is_empty()
        {
            bail!("invalid selected presentation scope");
        }
        if scope.scope_id == d.presentation_scene_id {
            if let Some(asset) = scope.assets.get(&d.source_text_binding) {
                selections.push(asset.clone());
            }
        }
    }
    {
        let selected = &d.source_text;
        if selections
            .iter()
            .filter(|asset| {
                asset.sha256 == selected.sha256
                    && asset.bytes == selected.bytes
                    && asset.cache_uri == format!("cache://sha256/{}", selected.sha256)
                    && matches!(
                        asset.selection_state.as_str(),
                        "selected-private-production" | "principal-approved" | "release-cleared"
                    )
            })
            .count()
            != 1
        {
            bail!("presentation source text lacks one exact selected scoped binding");
        }
    }
    if !matches!(template.kind.as_str(), "chapter-title" | "semantic-label")
        || d.invocation.language != inputs.manifest.language
        || d.presentation_scene_id != inputs.manifest.scene_id
        || template.canvas_width != original_job.width
        || template.canvas_height != original_job.height
    {
        bail!("presentation template has foreign language, kind or canvas");
    }
    verify_source_text(
        &d.invocation,
        &source,
        &d.source_authority_id,
        &d.source_scope_ids,
        &[],
    )?;
    let contract: crate::cue_relative::Contract = read(&scene_delivery::checked_file(
        overlay
            .original
            .job_path
            .parent()
            .context("original parent")?,
        &original_job.contract,
    )?)?;
    let semantic: crate::semantic_delivery::SemanticDelivery = read(
        &scene_delivery::checked_file(root, &inputs.manifest.semantic_delivery)?,
    )?;
    let mut clocks = BTreeMap::new();
    let mut ordered = Vec::new();
    let mut cursor = 0u64;
    if d.cue_bindings.len() != contract.cues.len() {
        bail!("presentation cue bindings omit original clocks");
    }
    for (binding, clock) in d.cue_bindings.iter().zip(&contract.cues) {
        if binding.native_cue_id != clock.cue_id || binding.source_cue_id.trim().is_empty() {
            bail!("presentation clock order differs from original cues");
        }
        let audio = original_job
            .audio
            .iter()
            .find(|a| a.bus == "D" && a.cue_id.as_deref() == Some(&clock.cue_id))
            .context("presentation native cue has no selected D")?;
        let native_span = original_plan
            .audio
            .iter()
            .find(|span| span.attachment_id == audio.attachment_id)
            .context("presentation native D span missing")?;
        let end = cursor
            .checked_add(clock.duration_samples)
            .context("presentation clock overflow")?;
        if native_span.start_sample != cursor || native_span.end_sample != end {
            bail!("presentation reconstructed cue clock differs from original D span");
        }
        cursor = end;
        if !source_alias_owns_logical_id(&binding.source_cue_id, &binding.narration_logical_id) {
            bail!("presentation source alias does not own original narration logical ID");
        }
        let witnessed = semantic.event_bindings.iter().any(|b| {
            b.narration_attachment_id == audio.attachment_id
                && semantic.graph.events.iter().any(|e| {
                    e.event_id == b.event_id
                        && e.narration.logical_id == binding.narration_logical_id
                        && e.narration.sha256 == audio.source.sha256
                })
        });
        if !witnessed {
            bail!("presentation source alias lacks selected semantic narration identity");
        }
        ordered.push(binding.source_cue_id.clone());
        if clocks
            .insert(
                binding.source_cue_id.clone(),
                NativeAlignment {
                    schema: reel_assembly::scene_authoring::NATIVE_ALIGNMENT_SCHEMA.into(),
                    language: inputs.manifest.language.clone(),
                    cue_id: binding.source_cue_id.clone(),
                    selected_take_sha256: audio.source.sha256.clone(),
                    sample_rate: contract.sample_rate,
                    cue_end_sample: clock.duration_samples,
                    semantic_markers: BTreeMap::from([("line-entry".into(), 0)]),
                },
            )
            .is_some()
        {
            bail!("duplicate presentation cue alias");
        }
    }
    // First version admits native cue entrance only; word/phrase overlays need
    // independently measured selected marker evidence, not authored seconds.
    if d.invocation
        .lines
        .iter()
        .any(|l| l.semantic_trigger_id != "line-entry")
    {
        bail!("presentation successor requires an original native cue entrance");
    }
    let compiled = if template.kind == "chapter-title" {
        compile_layer(&template, &d.invocation, &[], &BTreeMap::new())?
    } else {
        compile_layer(&template, &d.invocation, &ordered, &clocks)?
    };
    if template.kind == "chapter-title" {
        let first_audio = original_job
            .audio
            .iter()
            .find(|a| {
                a.bus == "D"
                    && a.cue_id.as_deref() == contract.cues.first().map(|c| c.cue_id.as_str())
            })
            .context("chapter first prose D missing")?;
        let first_binding = semantic
            .event_bindings
            .iter()
            .find(|b| b.narration_attachment_id == first_audio.attachment_id)
            .context("chapter first prose source event missing")?;
        let picture = original_plan
            .pictures
            .iter()
            .find(|p| p.attachment_id == first_binding.picture_attachment_id)
            .context("chapter first prose picture missing")?;
        let audio = original_plan
            .audio
            .iter()
            .find(|p| p.attachment_id == first_audio.attachment_id)
            .context("chapter first prose audio missing")?;
        let visible = u128::from(compiled.duration_samples) * u128::from(original_plan.sample_rate);
        if audio.start_sample != 0
            || picture.start_sample != 0
            || visible > u128::from(audio.end_sample) * u128::from(compiled.sample_rate)
            || visible > u128::from(picture.end_sample) * u128::from(compiled.sample_rate)
        {
            bail!("chapter title exceeds its first prose picture or D span");
        }
    }
    if (template.kind == "semantic-label"
        && (compiled.duration_samples != original_plan.duration_samples
            || compiled.sample_rate != original_plan.sample_rate))
        || u128::from(compiled.duration_samples) * u128::from(original_plan.sample_rate)
            > u128::from(original_plan.duration_samples) * u128::from(compiled.sample_rate)
    {
        bail!("presentation compiled clocks differ from original scene");
    }
    let ass_path = scene_delivery::checked_file(assets, &d.ass)?;
    scene_delivery::checked_file(assets, &d.font)?;
    if fs::read(ass_path)? != compiled.ass.as_bytes() {
        bail!("presentation ASS differs from exact template/native-clock recompilation");
    }
    let receipt: Value = read(&scene_delivery::checked_file(root, &d.compile_receipt)?)?;
    for (key, expected) in [
        (
            "schema",
            serde_json::json!("reel.editable-layer-compile-receipt.v1"),
        ),
        ("scene_id", serde_json::json!(d.presentation_scene_id)),
        ("language", serde_json::json!(inputs.manifest.language)),
        ("template_id", serde_json::json!(template.template_id)),
        (
            "template_definition_sha256",
            serde_json::json!(d.template_definition.sha256),
        ),
        (
            "source_text_sha256",
            serde_json::json!(d.source_text.sha256),
        ),
        ("source_text_state", serde_json::json!(source.text_state)),
        ("ass_sha256", serde_json::json!(d.ass.sha256)),
        ("ass_bytes", serde_json::json!(d.ass.bytes)),
        ("sample_rate", serde_json::json!(original_plan.sample_rate)),
        (
            "duration_samples",
            serde_json::json!(original_plan.duration_samples),
        ),
    ] {
        if receipt.get(key) != Some(&expected) {
            bail!("presentation compile receipt disagrees at {key}");
        }
    }
    Ok(())
}

fn source_alias_owns_logical_id(alias: &str, logical: &str) -> bool {
    logical == alias
        || logical
            .strip_prefix(alias)
            .and_then(|suffix| suffix.chars().next())
            .is_some_and(|separator| matches!(separator, '.' | '-' | ':' | '/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_alias_requires_complete_original_identity_component() {
        assert!(source_alias_owns_logical_id(
            "B0520",
            "B0520-S01-ES-NARRATOR"
        ));
        assert!(source_alias_owns_logical_id("cue", "cue.wav"));
        assert!(source_alias_owns_logical_id("cue", "cue"));
        assert!(!source_alias_owns_logical_id(
            "B0520",
            "B0519-S01-ES-NARRATOR"
        ));
        assert!(!source_alias_owns_logical_id(
            "B052",
            "B0520-S01-ES-NARRATOR"
        ));
        assert!(!source_alias_owns_logical_id("", "B0520"));
    }
}
