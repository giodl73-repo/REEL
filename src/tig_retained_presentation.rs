//! Explicit composition of a selected delivered output. No output is a primary.
//! Metadata lowering grants neither original-producer currency nor publication.
use crate::{
    presentation_adopt::{Manifest, OutputMode},
    scene_delivery::{FileRef, InputFiles},
    tig_inputs::{Payload, Source},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::PathBuf,
};
use tig_assets::graph::{self, EdgeKind, NodeKind, Payloads, Profile, Reference};
use tig_core::{
    Digest, Object, identity,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::context::ProductionContext;
use tig_store::source_claims::Selection;

pub const SCHEMA: &str = "reel.tig-retained-presentation.experimental-1";
const CONTROL_MAX: usize = 1024 * 1024;
const TOTAL_MAX: usize = 4 * CONTROL_MAX;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub root_id: String,
    pub root_definition: String,
    /// Explicitly admitted delivered-output types, never production primaries.
    pub source_definitions: BTreeSet<String>,
}
impl Request {
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let mut raw = Vec::new();
        std::fs::File::open(path)?
            .take(65_537)
            .read_to_end(&mut raw)?;
        if raw.len() > 65_536 {
            bail!("retained request exceeds bounds");
        }
        Ok(serde_json::from_value(identity::parse_json(&raw)?)?)
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
    source: Id,
    language: String,
    episode_id: String,
    role: String,
    adoption_slot: String,
}

pub struct Prepared {
    pub root: Reference,
    pub source: Reference,
    pub source_file: FileRef,
    pub manifest_file: FileRef,
    pub manifest: Manifest,
    controls: BTreeMap<PathBuf, (Object, Vec<u8>)>,
}
impl Prepared {
    pub fn summary(&self) -> Value {
        json!({"schema":"reel.tig-retained-presentation-plan.experimental-1",
            "root":self.root,"source":self.source,"selected":{
                "sha256":self.source_file.sha256,"bytes":self.source_file.bytes},
            "role":self.manifest.role,"language":self.manifest.language,
            "episode_id":self.manifest.episode_id,"control_files":self.controls.len(),
            "media_verified":false,"primary_reference_verified":false,
            "original_producer_assertion":false,"executable":false,"approval":false})
    }
    pub fn metadata(&self) -> impl Iterator<Item = (&Object, &[u8])> {
        self.controls
            .values()
            .map(|(pin, raw)| (pin, raw.as_slice()))
    }
}
impl InputFiles for Prepared {
    fn verified_path(&self, _: &FileRef) -> Result<PathBuf> {
        bail!("retained metadata does not provide media paths")
    }
    fn control_bytes(&self, file: &FileRef) -> Result<Vec<u8>> {
        crate::scene_delivery::control_name(&file.path)?;
        let (pin, bytes) = self
            .controls
            .get(&file.path)
            .context("unbound retained control")?;
        if pin.sha256.as_str() != file.sha256 || pin.bytes != file.bytes {
            bail!("retained control differs from its exact pin");
        }
        Ok(bytes.clone())
    }
}
fn token(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        bail!("invalid retained presentation token");
    }
    Ok(())
}
fn pin(file: &FileRef) -> Result<Object> {
    Ok(Object {
        sha256: Digest::parse(&file.sha256)?,
        bytes: file.bytes,
    })
}
fn insert(
    controls: &mut BTreeMap<PathBuf, (Object, Vec<u8>)>,
    file: &FileRef,
    raw: Vec<u8>,
) -> Result<()> {
    crate::scene_delivery::control_name(&file.path)?;
    let object = pin(file)?;
    if Object::from_bytes(&raw) != object {
        bail!("retained control bytes differ");
    }
    if let Some((previous, _)) = controls.get(&file.path) {
        if previous != &object {
            bail!("conflicting retained control aliases");
        }
        return Ok(());
    }
    if controls.len() >= 12
        || raw.len() > CONTROL_MAX
        || controls.values().map(|(_, b)| b.len()).sum::<usize>() + raw.len() > TOTAL_MAX
    {
        bail!("retained metadata exceeds bounds");
    }
    controls.insert(file.path.clone(), (object, raw));
    Ok(())
}

/// Resolve declared output identity and bounded adoption controls. No media IO.
pub fn lower(source: &mut Source, request: &Request) -> Result<Prepared> {
    token(&request.root_id)?;
    token(&request.root_definition)?;
    if request.schema != SCHEMA
        || request.source_definitions.is_empty()
        || request.source_definitions.len() > 16
        || request
            .source_definitions
            .contains(&request.root_definition)
    {
        bail!("invalid retained presentation admission");
    }
    for id in &request.source_definitions {
        token(id)?;
        source.admission(id)?;
    }
    let root = source.record(&request.root_id)?;
    if root.definition().id != request.root_definition {
        bail!("retained root type differs");
    }
    let spec: Spec = serde_json::from_value(Value::Object(root.authored().clone()))?;
    for value in [
        &spec.source.id,
        &spec.language,
        &spec.episode_id,
        &spec.role,
        &spec.adoption_slot,
    ] {
        token(value)?;
    }
    if spec.source.id == request.root_id {
        bail!("retained source cannot alias its root");
    }
    let delivered = source.record(&spec.source.id)?;
    if !request
        .source_definitions
        .contains(&delivered.definition().id)
        || !matches!(
            source.admission(&delivered.definition().id)?.payload,
            Payload::Selected {}
        )
    {
        bail!("retained source is not an admitted selected output");
    }
    let selected = delivered.selected()?.clone();
    let adoption = root.stream(&spec.adoption_slot)?.clone();
    if adoption.bytes > CONTROL_MAX as u64 {
        bail!("retained adoption exceeds bounds");
    }
    let raw = source.read_metadata(&adoption, CONTROL_MAX)?;
    let manifest: Manifest = serde_json::from_value(identity::parse_json(&raw)?)?;
    if manifest.schema != crate::presentation_adopt::SCHEMA
        || manifest.output_mode != Some(OutputMode::RetainSource)
        || manifest.source_range.is_some()
        || manifest.evidence_range_pointer.is_some()
        || manifest.audio_clock_repair.is_some()
        || manifest.audio_range_origin.is_some()
        || manifest.role != spec.role
        || manifest.language != spec.language
        || manifest.episode_id != spec.episode_id
        || pin(&manifest.source)? != selected
    {
        bail!("retained adoption differs from occurrence or selected delivered output");
    }
    crate::scene_delivery::control_name(&manifest.source.path)?;
    let mut controls = BTreeMap::new();
    let manifest_file = FileRef {
        path: "adoption.json".into(),
        sha256: adoption.sha256.as_str().into(),
        bytes: adoption.bytes,
    };
    insert(&mut controls, &manifest_file, raw)?;
    for file in [
        &manifest.catalog,
        &manifest.template_definition,
        &manifest.season_bindings,
        &manifest.episode_bindings,
    ]
    .into_iter()
    .chain(manifest.source_template.iter())
    .chain(manifest.selection_evidence.iter())
    {
        let object = pin(file)?;
        if object.bytes > CONTROL_MAX as u64 {
            bail!("retained control exceeds bounds");
        }
        insert(
            &mut controls,
            file,
            source.read_metadata(&object, CONTROL_MAX)?,
        )?;
    }
    if controls.contains_key(&manifest.source.path) {
        bail!("retained media aliases metadata");
    }
    Ok(Prepared {
        root: root.reference().clone(),
        source: delivered.reference().clone(),
        source_file: FileRef {
            path: spec.source.id.into(),
            sha256: selected.sha256.as_str().into(),
            bytes: selected.bytes,
        },
        manifest_file,
        manifest,
        controls,
    })
}

fn limits() -> graph::Limits {
    graph::Limits {
        nodes: 1,
        edges: 1,
        depth: 1,
        bytes: 16 * 1024 * 1024,
    }
}
fn profiles(source: &Source, request: &Request, prepared: &Prepared) -> Result<Vec<Profile>> {
    let spec: Spec = serde_json::from_value(Value::Object(
        source.record(&prepared.root.id)?.authored().clone(),
    ))?;
    Ok(vec![Profile {
        definition_id: request.root_definition.clone(),
        definition: source.admission(&request.root_definition)?.object.clone(),
        class: NodeKind::Container,
        covered: BTreeSet::from([EdgeKind::Production]),
        rules: vec![],
        payloads: Payloads::Named {
            selected: false,
            slots: BTreeSet::from([spec.adoption_slot]),
        },
    }])
}
fn descriptor(source: &Source, request: &Request, prepared: &Prepared) -> Result<Vec<u8>> {
    let record = source.record(&prepared.source.id)?;
    // The delivered record is a composition obligation, not a Production node.
    Ok(identity::canonical_json(
        &json!({"schema":"reel.tig-retained-presentation-context.experimental-1",
        "request":request,"source":{"id":prepared.source.id,"definition":record.definition(),
            "selected":pin(&prepared.source_file)?},"adoption":pin(&prepared.manifest_file)?,
        "adapter":Object::from_bytes(include_bytes!("tig_retained_presentation.rs")),
        "adoption_kernel":Object::from_bytes(include_bytes!("presentation_adopt.rs"))}),
    )?)
}
pub fn freeze_context(
    source: &mut Source,
    request: &Request,
    store: &mut impl NodeStore,
    trees: TreeLimits,
) -> Result<ProductionContext> {
    let prepared = lower(source, request)?;
    let descriptor = descriptor(source, request, &prepared)?;
    source.check_native_context(&descriptor)?;
    source.freeze_context(
        &request.root_id,
        &profiles(source, request, &prepared)?,
        &descriptor,
        store,
        trees,
        limits(),
    )
}
/// Replay the same interpretation from retained metadata with Git unavailable.
pub fn restore_context(
    profile: crate::tig_inputs::ArchiveProfile,
    request: &Request,
    selection: &Selection,
    store: &mut impl NodeStore,
    trees: TreeLimits,
) -> Result<(Source, ProductionContext)> {
    let archive = profile.archive.clone();
    let native_limits = profile.limits;
    let mut source = Source::from_archive(profile, store, trees)?;
    let prepared = lower(&mut source, request)?;
    let descriptor = descriptor(&source, request, &prepared)?;
    let context = source.replay_context(
        selection,
        &profiles(&source, request, &prepared)?,
        store,
        native_limits,
        trees,
        limits(),
    )?;
    if context.archive() != &archive
        || context.descriptor() != descriptor
        || context.roots() != &BTreeSet::from([prepared.root])
    {
        bail!("retained replay differs from frozen interpretation");
    }
    source.bind_native_context(&descriptor);
    Ok((source, context))
}
pub fn recipe(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
) -> Result<graph::recipe::Plan> {
    let prepared = lower(source, request)?;
    let descriptor = descriptor(source, request, &prepared)?;
    source.check_native_context(&descriptor)?;
    if context.descriptor() != descriptor
        || context.requirements().checkpoint() != source.checkpoint()
        || context.roots() != &BTreeSet::from([prepared.root.clone()])
    {
        bail!("retained context differs");
    }
    let profiles = profiles(source, request, &prepared)?;
    let consumption = vec![graph::recipe::Consumption {
        definition_id: request.root_definition.clone(),
        definition: source.admission(&request.root_definition)?.object.clone(),
        fields: ["source", "language", "episode_id", "role", "adoption_slot"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }];
    let plan=source.production_recipe(&request.root_id,&profiles,&consumption,&graph::recipe::Operation {
        name:"reel.retained-output.composition.experimental-1".into(),
        semantics:Object::from_bytes(b"selected-delivered-output;exact-byte-reuse;not-primary;no-upstream-producer-claim").sha256,
        producer:producer.clone(),parameters:Object::from_bytes(&descriptor)},limits())?;
    if plan.admission() != context.requirements().admission()
        || plan.binding() != context.requirements().binding_digest()
    {
        bail!("retained recipe binding differs");
    }
    Ok(plan)
}
