//! Native still-sequence inputs compiled from one captured Tig metadata view.
//! Compilation does not read media, execute a producer or establish currency.
use crate::{
    presentation_still_sequence,
    scene_delivery::{self, FileRef, InputFiles, MediaProgram},
    tig_inputs::Source,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use tig_assets::graph::Reference;
use tig_core::{Digest, Object, identity};

pub const SCHEMA: &str = "reel.tig-presentation-source.experimental-1";
mod context;
pub use context::{freeze_context, recipe, requirements, restore_context};
#[cfg(feature = "tig-publication")]
pub mod jobs;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub presentation_id: String,
    pub source_definition: String,
    pub template_definition: String,
    pub timing_definition: String,
    pub clock_definition: String,
    pub evidence_definition: String,
    pub primary_definitions: BTreeSet<String>,
    pub allowed_primary_states: BTreeSet<String>,
}
impl Request {
    fn validate(&self) -> Result<()> {
        let definitions = [
            &self.source_definition,
            &self.template_definition,
            &self.timing_definition,
            &self.clock_definition,
            &self.evidence_definition,
        ];
        let mut unique = BTreeSet::new();
        if self.schema != SCHEMA
            || self.primary_definitions.is_empty()
            || self.primary_definitions.len() > 32
            || self.allowed_primary_states.is_empty()
            || self.allowed_primary_states.len() > 16
        {
            bail!("invalid presentation source admission");
        }
        token(&self.presentation_id)?;
        for value in definitions
            .into_iter()
            .chain(self.primary_definitions.iter())
        {
            token(value)?;
            if !unique.insert(value) {
                bail!("presentation source definitions overlap");
            }
        }
        for state in &self.allowed_primary_states {
            token(state)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    schema: String,
    episode_id: String,
    language: String,
    role: String,
    template: Id,
    pictures: Vec<Id>,
    editable_layer: Id,
    #[serde(default)]
    audio: Option<Id>,
    #[serde(default)]
    fonts: Vec<Id>,
    #[serde(default)]
    picture_timing: Option<Id>,
    #[serde(default)]
    audio_treatment: Option<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Timing {
    schema: String,
    clock: Id,
    spans: Vec<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clock {
    schema: String,
    source: Id,
    source_at_measurement: Object,
    sample_rate: u32,
    total_samples: u64,
    evidence: Id,
    anchors: Vec<Value>,
}

/// Exact literal metadata plus native primary-file pins from the captured view.
/// Metadata can be read without creating a filesystem source view. No public
/// constructor, execution/production-current/approval/eviction capability.
pub struct CompiledPresentation {
    checkpoint: Digest,
    root: Reference,
    records: BTreeMap<String, Reference>,
    controls: BTreeMap<PathBuf, (FileRef, Vec<u8>)>,
    primaries: BTreeMap<String, FileRef>,
    manifest: FileRef,
    bytes: u64,
}
impl CompiledPresentation {
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn root(&self) -> &Reference {
        &self.root
    }
    pub fn records(&self) -> impl Iterator<Item = &Reference> {
        self.records.values()
    }
    pub fn manifest(&self) -> &FileRef {
        &self.manifest
    }
    pub fn primary_files(&self) -> impl Iterator<Item = &FileRef> {
        self.primaries.values()
    }
    /// Hold exact primary byte/path guards from this same captured source.
    /// This is a synchronous input guard, not job/resource admission or custody.
    pub fn prepare(&self, source: &mut Source) -> Result<crate::tig_inputs::Inputs> {
        if source.checkpoint() != &self.checkpoint {
            bail!("compiled presentation belongs to another source checkpoint");
        }
        source.prepare_files(&self.primary_files().cloned().collect::<Vec<_>>())
    }
    pub fn metadata_files(&self) -> impl Iterator<Item = (&FileRef, &[u8])> {
        self.controls
            .values()
            .map(|(pin, bytes)| (pin, bytes.as_slice()))
    }
    fn insert(&mut self, name: PathBuf, raw: Vec<u8>) -> Result<FileRef> {
        scene_delivery::control_name(&name)?;
        if raw.len() > 4 * 1024 * 1024 || self.controls.len() >= 32 {
            bail!("presentation control budget exceeded");
        }
        self.bytes = self
            .bytes
            .checked_add(raw.len() as u64)
            .context("presentation metadata overflow")?;
        if self.bytes > 16 * 1024 * 1024 {
            bail!("presentation control budget exceeded");
        }
        let object = Object::from_bytes(&raw);
        let pin = FileRef {
            path: name.clone(),
            sha256: object.sha256.as_str().into(),
            bytes: object.bytes,
        };
        if let Some((old, bytes)) = self.controls.get(&name) {
            if old.sha256 != pin.sha256 || old.bytes != pin.bytes || bytes != &raw {
                bail!("presentation metadata name collision");
            }
        } else {
            self.controls.insert(name, (pin.clone(), raw));
        }
        Ok(pin)
    }
}
impl InputFiles for CompiledPresentation {
    fn verified_path(&self, _: &FileRef) -> Result<PathBuf> {
        bail!("presentation metadata has no filesystem fallback")
    }
    fn control_bytes(&self, pin: &FileRef) -> Result<Vec<u8>> {
        let (expected, raw) = self
            .controls
            .get(&pin.path)
            .context("presentation control is not compiled")?;
        if expected.sha256 != pin.sha256 || expected.bytes != pin.bytes {
            bail!("compiled presentation control pin changed");
        }
        Ok(raw.clone())
    }
    fn media_program(&self, _: MediaProgram) -> Result<PathBuf> {
        bail!("presentation metadata cannot supply media programs")
    }
}

fn token(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        bail!("invalid presentation source identity");
    }
    Ok(())
}
fn name(id: &str, kind: &str) -> PathBuf {
    format!("{kind}/{}", Digest::blob(id.as_bytes()).as_str()).into()
}
fn document(
    source: &Source,
    result: &mut CompiledPresentation,
    id: &str,
    definition: &str,
) -> Result<Value> {
    let record = source.record(id)?;
    if record.definition().id != definition {
        bail!("presentation control definition differs from admission");
    }
    result.records.insert(id.into(), record.reference().clone());
    let document = record
        .authored()
        .get("document")
        .cloned()
        .context("presentation authored document missing")?;
    if document.get("schema").and_then(Value::as_str)
        == Some("reel.presentation-clock-evidence-reference.v1")
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EvidenceReference {
            schema: String,
            original: Object,
        }
        let reference: EvidenceReference = serde_json::from_value(document.clone())?;
        if reference.schema != "reel.presentation-clock-evidence-reference.v1"
            || record.selected()? != &reference.original
        {
            bail!("clock evidence asset differs from its declared reference");
        }
    }
    Ok(document)
}
fn primary(
    source: &Source,
    request: &Request,
    result: &mut CompiledPresentation,
    id: &Id,
) -> Result<Value> {
    let record = source.record(&id.id)?;
    if !request
        .primary_definitions
        .contains(&record.definition().id)
    {
        bail!("presentation input is not an admitted primary");
    }
    let asset = source.resolve_asset(&id.id)?;
    if !request
        .allowed_primary_states
        .contains(&asset.selection_state)
    {
        bail!("presentation primary selection state is not admitted");
    }
    let pin = FileRef {
        path: id.id.clone().into(),
        sha256: asset.sha256,
        bytes: asset.bytes,
    };
    result
        .records
        .insert(id.id.clone(), record.reference().clone());
    result.primaries.insert(id.id.clone(), pin.clone());
    Ok(json!({"path":pin.path,"sha256":pin.sha256,"bytes":pin.bytes,"cache_uri":asset.cache_uri}))
}
fn control(result: &mut CompiledPresentation, id: &str, value: &Value) -> Result<Value> {
    let pin = result.insert(name(id, "control"), identity::canonical_json(value)?)?;
    Ok(
        json!({"path":pin.path,"sha256":pin.sha256,"bytes":pin.bytes,"cache_uri":format!("cache://sha256/{}",pin.sha256)}),
    )
}

/// Compile logical source IDs and literal native controls. File pins always
/// resolve from the current captured view; no second caller-authored hash list.
/// A measured clock retains its original audio object and refuses a new take
/// until the clock itself is updated. This method opens zero media bytes.
pub fn compile(source: &Source, request: &Request) -> Result<CompiledPresentation> {
    request.validate()?;
    let root = source.record(&request.presentation_id)?;
    if root.definition().id != request.source_definition {
        bail!("presentation source definition differs from admission");
    }
    let mut result = CompiledPresentation {
        checkpoint: source.checkpoint().clone(),
        root: root.reference().clone(),
        records: BTreeMap::from([(request.presentation_id.clone(), root.reference().clone())]),
        controls: BTreeMap::new(),
        primaries: BTreeMap::new(),
        manifest: FileRef {
            path: "native/manifest.json".into(),
            sha256: String::new(),
            bytes: 0,
        },
        bytes: 0,
    };
    let spec: Spec = serde_json::from_value(
        root.authored()
            .get("document")
            .cloned()
            .context("presentation authored source missing")?,
    )?;
    if spec.schema != "reel.tig-still-sequence-source.v1"
        || spec.pictures.is_empty()
        || spec.pictures.len() > 120
        || spec.fonts.len() > 16
    {
        bail!("invalid native presentation source");
    }
    token(&spec.episode_id)?;
    token(&spec.role)?;
    let template = document(
        source,
        &mut result,
        &spec.template.id,
        &request.template_definition,
    )?;
    let template = control(&mut result, &spec.template.id, &template)?;
    let pictures = spec
        .pictures
        .iter()
        .map(|id| primary(source, request, &mut result, id))
        .collect::<Result<Vec<_>>>()?;
    let layer = primary(source, request, &mut result, &spec.editable_layer)?;
    let fonts = spec
        .fonts
        .iter()
        .map(|id| primary(source, request, &mut result, id))
        .collect::<Result<Vec<_>>>()?;
    let audio = spec
        .audio
        .as_ref()
        .map(|id| primary(source, request, &mut result, id))
        .transpose()?;
    let timing = if let Some(id) = &spec.picture_timing {
        let timing: Timing = serde_json::from_value(document(
            source,
            &mut result,
            &id.id,
            &request.timing_definition,
        )?)?;
        let clock: Clock = serde_json::from_value(document(
            source,
            &mut result,
            &timing.clock.id,
            &request.clock_definition,
        )?)?;
        if spec
            .audio
            .as_ref()
            .is_none_or(|id| id.id != clock.source.id)
        {
            bail!("presentation clock belongs to another recording");
        }
        let selected = source.resolve_asset(&clock.source.id)?;
        if clock.source_at_measurement.sha256.as_str() != selected.sha256
            || clock.source_at_measurement.bytes != selected.bytes
        {
            bail!("presentation source clock is stale for selected recording");
        }
        let evidence = document(
            source,
            &mut result,
            &clock.evidence.id,
            &request.evidence_definition,
        )?;
        let evidence = control(&mut result, &clock.evidence.id, &evidence)?;
        let native_clock = json!({"schema":clock.schema,"source_sha256":clock.source_at_measurement.sha256,"sample_rate":clock.sample_rate,"total_samples":clock.total_samples,"evidence":evidence,"anchors":clock.anchors});
        let clock_ref = control(&mut result, &timing.clock.id, &native_clock)?;
        Some(control(
            &mut result,
            &id.id,
            &json!({"schema":timing.schema,"clock":clock_ref,"spans":timing.spans}),
        )?)
    } else {
        None
    };
    let mut manifest = json!({"schema":if timing.is_some() {"reel.presentation-still-sequence.v2"} else {"reel.presentation-still-sequence.v1"},"episode_id":spec.episode_id,"language":spec.language,"role":spec.role,"template":template,"pictures":pictures,"editable_layer":layer,"fonts":fonts});
    if let Some(audio) = audio {
        manifest["audio"] = audio;
    }
    if let Some(timing) = timing {
        manifest["picture_timing"] = timing;
    }
    if let Some(treatment) = spec.audio_treatment {
        manifest["audio_treatment"] = treatment;
    }
    result.manifest = result.insert(
        "native/manifest.json".into(),
        identity::canonical_json(&manifest)?,
    )?;
    presentation_still_sequence::validate_metadata(
        &result.control_bytes(&result.manifest)?,
        &result,
    )?;
    Ok(result)
}
