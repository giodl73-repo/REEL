//! Spoken interpretation over existing native context/recipe/publication ports.
//! No cache initialization, automatic selection, restoration, deletion or approval.
use super::{
    checker,
    compiler::{self, CompilationProfile, CompiledPlan, CompilerIdentity},
    producer::{self, ProcessingPolicy},
    *,
};
use crate::{
    scene_delivery::InputFiles,
    tig_inputs::{ArchiveProfile, Inputs, Payload, Source},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tig_assets::graph::{self, EdgeKind, NodeKind, Payloads};
use tig_core::{
    Object, identity,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::context::ProductionContext;
use tig_store::source_claims::Selection;

mod candidate;
mod retained;
pub use candidate::{NativeCandidate, NativeReceipt};
pub use retained::reopen;

pub const SCHEMA: &str = "reel.spoken-native-history.v1";
const OPERATION: &str = "reel.spoken-edition.produce.v1";
const SEMANTICS:&[u8]=b"reel.spoken-edition.produce.v1:exact-pcm24;selected-cue-payloads;per-edition-parameters;separate-full-context;no-complete-semantic-coverage";
const MAX_FILES: usize = 512;
const MAX_RETAINED_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub declaration: FileRef,
    pub producer: FileRef,
    pub processing: Option<FileRef>,
    pub profile: CompilationProfile,
    pub profiles: Vec<graph::Profile>,
}

struct Prepared {
    document: Document,
    processing: ProcessingPolicy,
    runtime: CompilerIdentity,
    plan: CompiledPlan,
    plan_bytes: Vec<u8>,
    inputs: Inputs,
    files: BTreeMap<String, FileRef>,
    descriptors: BTreeMap<String, Vec<u8>>,
    index_definitions: BTreeSet<String>,
}

/// Holds protected original inputs for the complete synchronous operation.
/// Metadata roots still require caller finish/readback and explicit adoption.
pub struct Frozen {
    request: Request,
    prepared: Prepared,
    contexts: BTreeMap<String, ProductionContext>,
    recipes: BTreeMap<String, graph::recipe::Plan>,
}
impl Frozen {
    pub fn context(&self, edition: &str) -> Result<&ProductionContext> {
        self.contexts
            .get(edition)
            .context("spoken native context missing")
    }
    pub fn plan_bytes(&self) -> &[u8] {
        &self.prepared.plan_bytes
    }
    pub fn recipe(&self, edition: &str) -> Result<&graph::recipe::Plan> {
        self.recipes
            .get(edition)
            .context("spoken native edition missing")
    }
    pub fn produce(&self, output: &Path) -> Result<producer::ProductionReceipt> {
        producer::produce(
            &self.prepared.document,
            &self.prepared.plan_bytes,
            &self.prepared.processing,
            &self.prepared.inputs,
            output,
            || Ok(()),
        )
    }
    pub fn check_candidate<'a>(
        &'a self,
        candidate: &Path,
        access: &Path,
    ) -> Result<NativeCandidate<'a>> {
        candidate::check(self, candidate, access)
    }
    pub fn write_access(&self, candidate: &Path, output: &Path) -> Result<()> {
        let receipt = producer::bounded(&candidate.join("receipt.json"), 16 * 1024 * 1024)?;
        let mut checked = checker::check(
            checker::CheckRequest {
                document: &self.prepared.document,
                plan_bytes: &self.prepared.plan_bytes,
                receipt_bytes: &receipt,
                processing: &self.prepared.processing,
                expected_runtime: &self.prepared.runtime,
            },
            &self.prepared.inputs,
            candidate,
            producer::ProductionControl {
                timeout: std::time::Duration::from_secs(30),
                cancellation: || Ok(()),
            },
        )?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        checker::write_access(&mut checked, output, || {
            require(
                std::time::Instant::now() < deadline,
                "spoken native text deadline elapsed",
            )
        })
    }
}

fn limits() -> graph::Limits {
    graph::Limits {
        nodes: 1024,
        edges: 1024,
        depth: 4,
        bytes: 16 * 1024 * 1024,
    }
}
fn object(file: &FileRef) -> Result<Object> {
    Ok(Object {
        sha256: tig_core::Digest::parse(&file.sha256)
            .map_err(|_| anyhow::anyhow!("spoken native object digest invalid"))?,
        bytes: file.bytes,
    })
}
fn put_file(files: &mut BTreeMap<String, FileRef>, file: &FileRef) -> Result<()> {
    let name = file
        .path
        .to_str()
        .context("spoken native input ID invalid")?;
    require(
        !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control),
        "spoken native input ID invalid",
    )?;
    if let Some(previous) = files.insert(name.into(), file.clone()) {
        require(
            previous.sha256 == file.sha256 && previous.bytes == file.bytes,
            "spoken native input pins conflict",
        )?;
    }
    require(
        files.len() <= MAX_FILES,
        "spoken native input count exceeded",
    )
}
fn cue_files(cue: &compiler::ConsumedCue) -> [&FileRef; 4] {
    [
        &cue.performance.source,
        &cue.performance.alignment,
        &cue.script.authority,
        &cue.performance.scope.evidence,
    ]
}
fn check_profiles(source: &Source, request: &Request) -> Result<()> {
    require(
        request.schema == SCHEMA && !request.profiles.is_empty() && request.profiles.len() <= 16,
        "spoken native request/profile count invalid",
    )?;
    let mut ids = BTreeSet::new();
    for profile in &request.profiles {
        let admission = source
            .admission(&profile.definition_id)
            .map_err(|_| anyhow::anyhow!("spoken native definition not admitted"))?;
        let expected = match &admission.payload {
            Payload::Selected {} => Payloads::SelectedObject,
            Payload::Stream { slot } => Payloads::Named {
                selected: false,
                slots: BTreeSet::from([slot.clone()]),
            },
        };
        require(
            ids.insert(&profile.definition_id)
                && profile.definition == admission.object
                && matches!(profile.class, NodeKind::Primary | NodeKind::Container)
                && profile.rules.is_empty()
                && profile.covered == BTreeSet::from([EdgeKind::Production])
                && serde_json::to_value(&profile.payloads)? == serde_json::to_value(expected)?,
            "spoken native profile differs from explicit leaf admission",
        )?;
    }
    Ok(())
}
fn prepare(source: &mut Source, request: &Request) -> Result<Prepared> {
    require(cfg!(windows), "spoken native protected inputs unsupported")?;
    check_profiles(source, request)?;
    require(
        request.declaration.bytes <= MAX_DECLARATION_BYTES as u64
            && request.producer.bytes > 0
            && request.producer.bytes <= 128 * 1024 * 1024
            && request
                .processing
                .as_ref()
                .is_none_or(|p| p.bytes <= MAX_DECLARATION_BYTES as u64),
        "spoken native control/runtime budget exceeded",
    )?;
    let mut controls = vec![request.declaration.clone()];
    if let Some(policy) = &request.processing {
        controls.push(policy.clone());
    }
    let preliminary = source
        .prepare_files(&controls)
        .map_err(|_| anyhow::anyhow!("spoken native protected controls unavailable"))?;
    let raw = preliminary
        .control_bytes(&request.declaration)
        .map_err(|_| anyhow::anyhow!("spoken native declaration unavailable"))?;
    require(
        object(&request.declaration)? == Object::from_bytes(&raw),
        "spoken native declaration pin differs",
    )?;
    let document: Document = serde_json::from_slice(&raw)
        .map_err(|_| anyhow::anyhow!("spoken native declaration JSON invalid"))?;
    validate(&document)?;
    let processing = if let Some(reference) = &request.processing {
        let bytes = preliminary
            .control_bytes(reference)
            .map_err(|_| anyhow::anyhow!("spoken native processing unavailable"))?;
        require(
            object(reference)? == Object::from_bytes(&bytes),
            "spoken native processing pin differs",
        )?;
        producer::parse_processing(&bytes)?
    } else {
        ProcessingPolicy::identity()
    };
    let mut files = BTreeMap::new();
    put_file(&mut files, &request.declaration)?;
    put_file(&mut files, &request.producer)?;
    if let Some(policy) = &request.processing {
        put_file(&mut files, policy)?;
    }
    // Only selected performances are retained/read. The aggregate registry is
    // provenance, never a reason to read an unselected language or take.
    for edition in &document.editions {
        for selection in &edition.selections {
            for take in &selection.takes {
                let p = document
                    .performances
                    .iter()
                    .find(|p| p.id == take.performance_id)
                    .context("spoken native performance missing")?;
                let script = document
                    .modules
                    .iter()
                    .find(|m| m.id == p.module_id)
                    .and_then(|m| m.cues.iter().find(|c| c.id == p.cue_id))
                    .and_then(|c| c.scripts.iter().find(|s| s.id == p.script_id))
                    .context("spoken native script missing")?;
                for file in [
                    &p.source,
                    &p.alignment,
                    &script.authority,
                    &p.scope.evidence,
                ] {
                    put_file(&mut files, file)?;
                }
            }
        }
    }
    let mut sizes = BTreeMap::new();
    for file in files.values() {
        let pin = object(file)?;
        require(
            sizes
                .insert(pin.sha256, pin.bytes)
                .is_none_or(|old| old == pin.bytes),
            "spoken native object size conflict",
        )?;
    }
    let total = sizes
        .values()
        .try_fold(0u64, |sum, n| sum.checked_add(*n))
        .context("spoken native input budget overflow")?;
    require(
        total <= MAX_RETAINED_BYTES,
        "spoken native retained input budget exceeded",
    )?;
    let inputs = source
        .prepare_files(&files.values().cloned().collect::<Vec<_>>())
        .map_err(|_| anyhow::anyhow!("spoken native protected inputs unavailable"))?;
    let runtime = CompilerIdentity {
        sha256: request.producer.sha256.clone(),
        bytes: request.producer.bytes,
    };
    let plan = compiler::compile_with_inputs(&document, &inputs, &runtime, request.profile)?;
    producer::validate_processing(&processing, &plan)?;
    let plan_bytes = serde_json::to_vec_pretty(&plan)?;
    let mut index_definitions = BTreeSet::new();
    for file in std::iter::once(&request.declaration).chain(request.processing.iter()) {
        let record = source
            .record(
                file.path
                    .to_str()
                    .context("spoken native control ID invalid")?,
            )
            .map_err(|_| anyhow::anyhow!("spoken native index record unavailable"))?;
        index_definitions.insert(record.definition().id.clone());
    }
    for id in &index_definitions {
        require(
            request.profiles.iter().any(|profile| {
                &profile.definition_id == id && profile.class == NodeKind::Container
            }),
            "spoken native index definition must be an admitted container",
        )?;
    }
    for file in files.values().filter(|file| {
        file.path != request.declaration.path
            && request
                .processing
                .as_ref()
                .is_none_or(|p| p.path != file.path)
    }) {
        let record = source
            .record(
                file.path
                    .to_str()
                    .context("spoken native input ID invalid")?,
            )
            .map_err(|_| anyhow::anyhow!("spoken native input record unavailable"))?;
        require(
            !index_definitions.contains(&record.definition().id),
            "spoken native index and consumed payload definitions must differ",
        )?;
    }
    let mut descriptors = BTreeMap::new();
    for edition in &plan.editions {
        let operations = edition
            .recipe
            .cues
            .iter()
            .map(|cue| {
                producer::operation(&processing, &edition.recipe.edition_id, &cue.performance.id)
            })
            .collect::<Vec<_>>();
        let descriptor = identity::canonical_json(
            &serde_json::json!({"schema":"reel.spoken-native-consumption.v1",
            "recipe":edition.recipe,"processing":operations}),
        )?;
        require(
            descriptor.len() <= 65536,
            "spoken native descriptor exceeds budget",
        )?;
        source
            .check_native_context(&descriptor)
            .map_err(|_| anyhow::anyhow!("spoken native frozen descriptor differs"))?;
        descriptors.insert(edition.recipe.edition_id.clone(), descriptor);
    }
    Ok(Prepared {
        document,
        processing,
        runtime,
        plan,
        plan_bytes,
        inputs,
        files,
        descriptors,
        index_definitions,
    })
}
fn edition_roots(
    request: &Request,
    edition: &compiler::CompiledEdition,
) -> Result<BTreeSet<String>> {
    let mut roots = BTreeSet::from([
        request
            .producer
            .path
            .to_str()
            .context("spoken native runtime ID invalid")?
            .to_owned(),
        request
            .declaration
            .path
            .to_str()
            .context("spoken native declaration ID invalid")?
            .to_owned(),
    ]);
    if let Some(policy) = &request.processing {
        roots.insert(
            policy
                .path
                .to_str()
                .context("spoken native policy ID invalid")?
                .into(),
        );
    }
    for cue in &edition.recipe.cues {
        for file in cue_files(cue) {
            roots.insert(
                file.path
                    .to_str()
                    .context("spoken native input ID invalid")?
                    .into(),
            );
        }
    }
    Ok(roots)
}
fn recipe_policies(
    request: &Request,
    prepared: &Prepared,
) -> (
    Vec<graph::recipe::Consumption>,
    Vec<graph::recipe::PayloadConsumption>,
) {
    let consumption = request
        .profiles
        .iter()
        .map(|p| graph::recipe::Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: BTreeSet::new(),
        })
        .collect::<Vec<_>>();
    let payloads = request
        .profiles
        .iter()
        .map(|p| {
            let (selected, slots) = if prepared.index_definitions.contains(&p.definition_id) {
                (false, BTreeSet::new())
            } else {
                match &p.payloads {
                    Payloads::SelectedObject => (true, BTreeSet::new()),
                    Payloads::Named { selected, slots } => (*selected, slots.clone()),
                    _ => unreachable!("profile admission checked"),
                }
            };
            graph::recipe::PayloadConsumption {
                definition_id: p.definition_id.clone(),
                definition: p.definition.clone(),
                selected,
                slots,
            }
        })
        .collect::<Vec<_>>();
    (consumption, payloads)
}
fn recipes(
    source: &mut Source,
    request: &Request,
    prepared: &Prepared,
) -> Result<BTreeMap<String, graph::recipe::Plan>> {
    let (consumption, payloads) = recipe_policies(request, prepared);
    let mut result = BTreeMap::new();
    for edition in &prepared.plan.editions {
        let roots = edition_roots(request, edition)?;
        let operation = graph::recipe::Operation {
            name: OPERATION.into(),
            semantics: Object::from_bytes(SEMANTICS).sha256,
            producer: object(&request.producer)?,
            parameters: Object::from_bytes(&prepared.descriptors[&edition.recipe.edition_id]),
        };
        let recipe = source
            .projected_recipe_for_ids(
                &roots,
                &request.profiles,
                &consumption,
                &payloads,
                &operation,
                limits(),
            )
            .map_err(|_| anyhow::anyhow!("spoken native consumed recipe unavailable"))?;
        result.insert(edition.recipe.edition_id.clone(), recipe);
    }
    Ok(result)
}

pub fn freeze(
    source: &mut Source,
    request: &Request,
    nodes: &mut impl NodeStore,
    trees: TreeLimits,
) -> Result<Frozen> {
    let prepared = prepare(source, request)?;
    let mut contexts = BTreeMap::new();
    for edition in &prepared.plan.editions {
        let id = &edition.recipe.edition_id;
        let context = source
            .freeze_context_for_ids(
                &edition_roots(request, edition)?,
                &request.profiles,
                &prepared.descriptors[id],
                nodes,
                trees,
                limits(),
            )
            .map_err(|_| anyhow::anyhow!("spoken native context freeze failed"))?;
        contexts.insert(id.clone(), context);
    }
    let recipes = recipes(source, request, &prepared)?;
    Ok(Frozen {
        request: request.clone(),
        prepared,
        contexts,
        recipes,
    })
}

pub fn restore(
    profile: ArchiveProfile,
    request: &Request,
    selections: &BTreeMap<String, Selection>,
    nodes: &mut impl NodeStore,
    trees: TreeLimits,
) -> Result<Frozen> {
    let archive = profile.archive.clone();
    let native = profile.limits;
    let mut source = Source::from_archive(profile, nodes, trees)
        .map_err(|_| anyhow::anyhow!("spoken native archive unavailable"))?;
    let prepared = prepare(&mut source, request)?;
    require(
        selections.len() == prepared.plan.editions.len(),
        "spoken native context count differs",
    )?;
    let mut contexts = BTreeMap::new();
    for edition in &prepared.plan.editions {
        let id = &edition.recipe.edition_id;
        let selection = selections
            .get(id)
            .context("spoken native original context missing")?;
        let stored = tig_git::source::native::context::read_descriptor(selection, nodes, trees)
            .map_err(|_| anyhow::anyhow!("spoken native context descriptor unavailable"))?;
        require(
            stored == prepared.descriptors[id],
            "spoken native restored controls/runtime differ",
        )?;
        let expected = source
            .production_requirements_for_ids(
                &edition_roots(request, edition)?,
                &request.profiles,
                limits(),
            )
            .map_err(|_| anyhow::anyhow!("spoken native restored scope unavailable"))?;
        let context = source
            .replay_context(selection, &request.profiles, nodes, native, trees, limits())
            .map_err(|_| anyhow::anyhow!("spoken native context replay failed"))?;
        require(
            context.archive() == &archive && context.requirements() == &expected,
            "spoken native context archive/scope differs",
        )?;
        contexts.insert(id.clone(), context);
    }
    let recipes = recipes(&mut source, request, &prepared)?;
    Ok(Frozen {
        request: request.clone(),
        prepared,
        contexts,
        recipes,
    })
}
