//! Metadata-only correspondence for a separately clocked chapter overlay.
use super::*;
use std::io::Read;

pub(super) struct Prepared {
    pub declaration: TargetedPresentation,
    source: Scene,
}
impl Prepared {
    pub fn read(
        root: &Path,
        declaration: Option<&TargetedPresentation>,
        target: &Scene,
        hashes: &mut BTreeMap<String, String>,
    ) -> Result<Option<Self>> {
        let Some(declaration) = declaration else {
            return Ok(None);
        };
        declaration.validate()?;
        let mut bytes = Vec::new();
        fs::File::open(checked(root, &declaration.scene)?)?
            .take(65_537)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            bail!("targeted presentation source exceeds metadata budget");
        }
        let source: Scene = serde_json::from_slice(&bytes)?;
        let target_presentation = target
            .presentation
            .as_ref()
            .context("targeted presentation requires a canonical presentation invocation")?;
        let presentation = source
            .presentation
            .as_ref()
            .context("presentation source has no invocation")?;
        if source.schema != "reel.scene-authoring.v1"
            || source.authoring_state != "ready-for-private-build"
            || source.scene_id.is_empty()
            || source.scene_id == target.scene_id
            || source.episode_id != target.episode_id
            || source.source_authority_id != target.source_authority_id
            || source.languages.is_empty()
            || source
                .languages
                .values()
                .any(|lane| !lane.cues.is_empty() || !lane.events.is_empty())
            || !source.shared_events.is_empty()
            || !source.language_event_bindings.is_empty()
            || presentation.role != "chapter-title"
            || target_presentation.role != "chapter-title"
            || presentation.template_id != target_presentation.template_id
            || presentation.template_kind != target_presentation.template_kind
            || presentation
                .asset_binding
                .as_ref()
                .is_some_and(|binding| target_presentation.asset_binding.as_ref() != Some(binding))
        {
            bail!("targeted presentation source differs from canonical invocation");
        }
        let mut canonical = target_presentation.content.clone();
        let canonical = canonical
            .as_object_mut()
            .context("invalid canonical presentation content")?;
        canonical.remove("ass_layer_bindings");
        canonical.remove("template_receipt_bindings");
        if presentation.content.as_object() != Some(canonical) {
            bail!("targeted presentation content differs from canonical invocation");
        }
        let placement = presentation
            .placement
            .as_ref()
            .and_then(Value::as_object)
            .context("targeted presentation placement missing")?;
        if placement.len() != 3
            || placement.get("mode") != Some(&json!("overlay-on-target-scene-start"))
            || placement.get("target_scene_id") != Some(&json!(target.scene_id))
        {
            bail!("targeted presentation placement differs from target scene");
        }
        hashes.insert("presentation_source".into(), hash(&bytes));
        Ok(Some(Self {
            declaration: declaration.clone(),
            source,
        }))
    }

    pub fn validate_receipt(
        &self,
        root: &Path,
        target: &Scene,
        context: PresentationReceiptContext<'_>,
    ) -> Result<()> {
        let PresentationReceiptContext {
            language,
            receipt_path,
            catalog,
            scopes,
            clock: (sample_rate, spoken_samples),
        } = context;
        if sample_rate == 0 || spoken_samples == 0 {
            bail!("targeted presentation requires a positive target clock");
        }
        let presentation = target
            .presentation
            .as_ref()
            .context("canonical presentation missing")?;
        let first = target
            .languages
            .get(language)
            .and_then(|lane| lane.events.first())
            .context("targeted presentation has no target picture")?;
        let source_presentation = self.source.presentation.as_ref().unwrap();
        if !self.source.languages.contains_key(language)
            || source_presentation.placement.as_ref().unwrap()["target_picture_slot_id"]
                != first.picture_slot_id
        {
            bail!("targeted presentation does not bind the first language picture");
        }
        let content = &presentation.content;
        let receipt_asset = asset(
            presentation_binding(content, "template_receipt_bindings", language)?,
            scopes,
        )?;
        let path = receipt_path.context("targeted presentation receipt missing")?;
        let mut bytes = Vec::new();
        fs::File::open(checked(root, path)?)?
            .take(65_537)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            bail!("targeted presentation receipt exceeds metadata budget");
        }
        if hash(&bytes) != receipt_asset.sha256 || bytes.len() as u64 != receipt_asset.bytes {
            bail!("targeted presentation receipt differs from selected binding");
        }
        let receipt: Value = serde_json::from_slice(&bytes)?;
        let template = catalog
            .templates
            .iter()
            .find(|value| value.template_id == presentation.template_id)
            .context("targeted presentation template missing")?;
        let layer = asset(
            presentation_binding(content, "ass_layer_bindings", language)?,
            scopes,
        )?;
        let text = asset(
            presentation_binding(content, "source_text_bindings", language)?,
            scopes,
        )?;
        if template.kind != "chapter-title"
            || receipt["schema"] != "reel.editable-layer-compile-receipt.v1"
            || receipt["scene_id"] != self.source.scene_id
            || receipt["language"] != language
            || receipt["template_id"] != template.template_id
            || receipt["template_definition_sha256"] != template.definition_sha256
            || receipt["ass_sha256"] != layer.sha256
            || receipt["ass_bytes"].as_u64() != Some(layer.bytes)
            || receipt["source_text_sha256"] != text.sha256
        {
            bail!("targeted presentation receipt identity differs from selected sources");
        }
        let rate = receipt["sample_rate"]
            .as_u64()
            .filter(|value| *value != 0)
            .context("targeted presentation receipt has no positive rate")?;
        let duration = receipt["duration_samples"]
            .as_u64()
            .filter(|value| *value != 0)
            .context("targeted presentation receipt has no positive duration")?;
        let scaled = duration
            .checked_mul(u64::from(sample_rate))
            .context("presentation clock overflow")?;
        if scaled % rate != 0 || scaled / rate > spoken_samples {
            bail!("targeted presentation cannot extend or fractionally retime the scene");
        }
        Ok(())
    }
}
