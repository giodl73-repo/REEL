//! Declared generation bases over Tig. Plans and unsigned claims are not model
//! execution, approval, byte availability, or authority to render or evict.
use crate::tig_inputs::{ArchiveProfile, Payload, Source};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tig_assets::graph::{self, Binding, Collection, EdgeKind, NodeKind, Payloads, Profile, Rule};
use tig_core::{
    Object, identity,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::{context::ProductionContext, produced};

const SEMANTICS: &[u8] = b"reel.native-generation-basis.v1:separate-spec-root;direct-primary-inputs;literal-profile-and-parameters;explicit-target;no-selected-output-consumption;no-execution";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub output_id: String,
    pub output_definition: String,
    pub spec_id: String,
    pub spec_definition: String,
    pub profile_definition: String,
    pub primary_definitions: BTreeSet<String>,
    pub operation: String,
    pub allowed_input_states: BTreeSet<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ref {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    schema: String,
    operation: String,
    inputs: BTreeMap<String, Ref>,
    output_profile: Ref,
    parameters: BTreeMap<String, Value>,
}
fn limits() -> graph::Limits {
    graph::Limits {
        nodes: 128,
        edges: 128,
        depth: 4,
        bytes: 16 * 1024 * 1024,
    }
}
fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn descriptor(request: &Request) -> Result<Vec<u8>> {
    let raw = identity::canonical_json(
        &json!({"schema":"reel.generation-context.v1","output_id":request.output_id,"output_definition":request.output_definition,"spec_id":request.spec_id,"operation":request.operation,"semantics":Object::from_bytes(SEMANTICS).sha256}),
    )?;
    if raw.len() > 65536 {
        bail!("generation descriptor exceeds budget");
    }
    Ok(raw)
}
fn validate(source: &Source, r: &Request) -> Result<()> {
    if r.schema != "reel.tig-generation-request.experimental-1"
        || [
            &r.output_id,
            &r.output_definition,
            &r.spec_id,
            &r.spec_definition,
            &r.profile_definition,
            &r.operation,
        ]
        .iter()
        .any(|s| !token(s))
        || r.output_id == r.spec_id
        || r.primary_definitions.is_empty()
        || !r.primary_definitions.contains(&r.output_definition)
        || r.primary_definitions.len() > 64
        || r.primary_definitions
            .iter()
            .any(|id| !token(id) || id == &r.spec_definition || id == &r.profile_definition)
        || r.spec_definition == r.profile_definition
        || r.allowed_input_states.is_empty()
        || r.allowed_input_states.len() > 64
        || r.allowed_input_states.iter().any(|s| !token(s))
    {
        bail!("invalid generation request");
    }
    let output = source.record(&r.output_id)?;
    if output.definition().id != r.output_definition
        || output.authored().get("generation_spec") != Some(&json!({"id":r.spec_id}))
    {
        bail!("output does not bind the admitted generation spec");
    }
    let spec = source.record(&r.spec_id)?;
    if spec.definition().id != r.spec_definition {
        bail!("generation spec definition differs");
    }
    let value = spec
        .authored()
        .get("document")
        .ok_or_else(|| anyhow::anyhow!("generation document missing"))?;
    let doc: Spec = serde_json::from_value(value.clone())?;
    if doc.schema != "reel.generation-spec.v1"
        || doc.operation != r.operation
        || doc.inputs.len() > 64
        || doc.inputs.keys().any(|k| !token(k))
        || doc.parameters.len() > 128
        || serde_json::to_vec(&doc.parameters)?.len() > 32768
    {
        bail!("unsupported generation spec");
    }
    for input in doc.inputs.values() {
        if !token(&input.id) || input.id == r.output_id {
            bail!("self generation requires an independently pinned reference input");
        }
        let record = source.record(&input.id)?;
        if !r.primary_definitions.contains(&record.definition().id)
            || !record
                .status()
                .get("selection_state")
                .and_then(Value::as_str)
                .is_some_and(|s| r.allowed_input_states.contains(s))
        {
            bail!("generation input type or state not admitted");
        }
    }
    let profile = source.record(&doc.output_profile.id)?;
    if profile.definition().id != r.profile_definition
        || !profile
            .authored()
            .get("document")
            .is_some_and(Value::is_object)
    {
        bail!("generation output profile not admitted");
    }
    Ok(())
}
fn profiles(source: &Source, r: &Request) -> Result<Vec<Profile>> {
    let rule = |pointer: &str, collection| Rule {
        pointer: pointer.into(),
        kind: EdgeKind::Production,
        collection,
        optional: false,
        binding: Binding::SnapshotSelected,
    };
    let mut result = Vec::new();
    for (id, rules) in [
        (
            &r.spec_definition,
            vec![
                rule("/document/inputs", Collection::ObjectValues),
                rule("/document/output_profile", Collection::One),
            ],
        ),
        (&r.profile_definition, vec![]),
    ] {
        result.push(Profile {
            definition_id: id.clone(),
            definition: source.admission(id)?.object.clone(),
            class: NodeKind::Container,
            covered: BTreeSet::from([EdgeKind::Production]),
            rules,
            payloads: Payloads::None,
        });
    }
    for id in &r.primary_definitions {
        let a = source.admission(id)?;
        result.push(Profile {
            definition_id: id.clone(),
            definition: a.object.clone(),
            class: NodeKind::Primary,
            covered: BTreeSet::from([EdgeKind::Production]),
            rules: vec![],
            payloads: match &a.payload {
                Payload::Selected {} => Payloads::SelectedObject,
                Payload::Stream { slot } => Payloads::Named {
                    selected: false,
                    slots: BTreeSet::from([slot.clone()]),
                },
            },
        });
    }
    Ok(result)
}
fn fields(profiles: &[Profile]) -> Vec<graph::recipe::Consumption> {
    profiles
        .iter()
        .map(|p| graph::recipe::Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: if p.class == NodeKind::Container {
                BTreeSet::from(["document".into()])
            } else {
                BTreeSet::new()
            },
        })
        .collect()
}
fn operation(r: &Request, producer: &Object) -> Result<graph::recipe::Operation> {
    // Eligibility is independent caller admission and is checked on every plan
    // and replay. Status-policy changes cannot change the generated byte request.
    Ok(graph::recipe::Operation {
        name: "reel.native-generation-basis.v1".into(),
        semantics: Object::from_bytes(SEMANTICS).sha256,
        producer: producer.clone(),
        parameters: Object::from_bytes(&descriptor(r)?),
    })
}
/// Metadata only. The output's selected bytes are deliberately outside this
/// recipe; selecting a result cannot change the request that produced it.
pub fn recipe(
    source: &mut Source,
    request: &Request,
    producer: &Object,
) -> Result<graph::recipe::Plan> {
    validate(source, request)?;
    let p = profiles(source, request)?;
    source.production_recipe(
        &request.spec_id,
        &p,
        &fields(&p),
        &operation(request, producer)?,
        limits(),
    )
}
/// Caller owns provider finish/readback, custody and actual producer admission.
pub fn freeze_context(
    source: &mut Source,
    request: &Request,
    store: &mut impl NodeStore,
    tree: TreeLimits,
) -> Result<ProductionContext> {
    validate(source, request)?;
    source.freeze_context(
        &request.spec_id,
        &profiles(source, request)?,
        &descriptor(request)?,
        store,
        tree,
        limits(),
    )
}
/// Replay an original declared generation basis under independent source and
/// producer admission. Never execute a model or infer approval from a match.
pub fn restore_claim<'a>(
    profile: ArchiveProfile,
    request: &Request,
    producer: &Object,
    claim: &'a graph::recipe::produced::Claim,
    store: &mut impl NodeStore,
    tree: TreeLimits,
) -> Result<(Source, produced::ReplayedClaim<'a>)> {
    let archive = profile.archive.clone();
    let native = profile.limits;
    let mut source = Source::from_archive(profile, store, tree)?;
    validate(&source, request)?;
    let output_definition = source.admission(&request.output_definition)?;
    if claim.outputs().len() != 1
        || claim.outputs()[0].asset_id() != request.output_id
        || claim.outputs()[0].definition().id != request.output_definition
        || claim.outputs()[0].definition().object != output_definition.object
        || claim.outputs()[0].selector() != &(graph::recipe::produced::Selector::Selected {})
    {
        bail!("generation claim does not bind the requested selected output");
    }
    let p = profiles(&source, request)?;
    let fields = fields(&p);
    let operation = operation(request, producer)?;
    if claim.operation() != &operation {
        bail!("generation operation differs from admission");
    }
    let q = limits();
    let admission = produced::Admission {
        profiles: &p,
        consumption: &fields,
        operation: &operation,
        native,
        tree,
        query: q,
        recipe: graph::recipe::Limits {
            nodes: q.nodes,
            edges: q.edges,
            bytes: q.bytes,
        },
    };
    let replay = source.replay_produced(claim, &admission, store)?;
    if replay.context().archive() != &archive
        || replay.context().descriptor() != descriptor(request)?
        || replay.context().roots()
            != &BTreeSet::from([source.record(&request.spec_id)?.reference().clone()])
    {
        bail!("generation claim source or request differs");
    }
    Ok((source, replay))
}

mod inspection;
pub use inspection::inspect;
pub mod advice;

/// Registered upstream generators only; absence is not proof of manual origin
/// or historical generation validity. No dependency payloads are opened.
pub fn upstream(source: &Source, request: &Request) -> Result<BTreeSet<String>> {
    validate(source, request)?;
    let record = source.record(&request.spec_id)?;
    let spec: Spec = serde_json::from_value(record.authored()["document"].clone())?;
    let mut result = BTreeSet::new();
    for input in spec.inputs.values() {
        let asset = source.record(&input.id)?;
        if let Some(value) = asset.authored().get("generation_spec") {
            let reference: Ref = serde_json::from_value(value.clone())?;
            if source.record(&reference.id)?.definition().id != request.spec_definition {
                bail!("upstream generation definition not admitted");
            }
            result.insert(input.id.clone());
        }
    }
    Ok(result)
}
