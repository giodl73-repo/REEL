//! One immutable private recipe, using the original journal prepare/adopt protocol.
use super::{
    codec,
    model::{self, Handle, Recipe},
};
use tig_core::{Digest, Object};
use tig_store::{
    Key,
    metadata::MetadataStore,
    source_claims::{Command, Selection, SourceJournal},
};

fn source(operation: &Digest) -> Result<Key, String> {
    Key::parse(&format!("retire-{}", operation.as_str()))
        .map_err(|_| "invalid retirement recipe source".into())
}
fn command(operation: &Digest, phase: &str) -> Result<Key, String> {
    Key::parse(&format!("retire-{phase}-{}", operation.as_str()))
        .map_err(|_| "invalid retirement recipe command".into())
}
fn context(root: &Object) -> Result<Digest, String> {
    Ok(Digest::blob(&codec::encode(&(
        "tig.native-retirement-controller-context.experimental-1",
        root,
    ))?))
}

/// Existing recipes are discovered before rebuilding dynamic projection facts.
/// Input equality is checked separately against the admitted semantic input ID.
pub fn existing(
    journal: &mut SourceJournal,
    nodes: &MetadataStore,
    operation: &Digest,
    owner: &Key,
) -> Result<Option<(Object, Recipe)>, String> {
    let id = source(operation)?;
    let Some(state) = journal
        .source(&id)
        .map_err(|_| "retirement recipe authority unavailable")?
    else {
        return Ok(None);
    };
    if &state.owner != owner || state.cancelled || !state.predecessors.is_empty() {
        return Err("foreign/cancelled retirement recipe authority".into());
    }
    let selection = match (&state.current, &state.pending) {
        (Some(current), None) if state.revision == 2 && current.revision == 2 => &current.selection,
        (None, Some(pending)) if state.revision == 1 => &pending.selection,
        _ => return Err("invalid retirement recipe state".into()),
    };
    if context(&selection.root)? != selection.context {
        return Err("retirement recipe selection integrity".into());
    }
    let bytes = codec::get(nodes, &selection.root)?;
    let recipe: Recipe = codec::decode(&bytes)?;
    recipe.authorities.validate()?;
    recipe.limits.validate()?;
    if !matches!(
        (
            recipe.schema.as_str(),
            recipe.policy_authority.as_ref(),
            recipe.controller_operation.as_ref()
        ),
        (model::RECIPE, None, None) | (model::POLICY_RECIPE, Some(_), Some(_))
    ) || recipe
        .controller_operation
        .as_ref()
        .unwrap_or(&recipe.operation)
        != operation
        || recipe.enrollment.owner != *owner
        || recipe.authorities.journal.identity != *journal.identity()
        || codec::encode(&recipe)? != bytes
    {
        return Err("retirement recipe binding/encoding mismatch".into());
    }
    Ok(Some((selection.root.clone(), recipe)))
}

/// Finish metadata before either journal acknowledgement. Exact retry never
/// rewrites an occupied recipe; an uncertain acknowledgement stays reconcilable.
pub fn save(
    journal: &mut SourceJournal,
    nodes: &mut MetadataStore,
    recipe: &Recipe,
) -> Result<Object, String> {
    if recipe.authorities.journal.identity != *journal.identity() {
        return Err("foreign recipe journal".into());
    }
    let identity = recipe
        .controller_operation
        .as_ref()
        .unwrap_or(&recipe.operation);
    let bytes = codec::encode(recipe)?;
    let root = Object::from_bytes(&bytes);
    let selection = Selection {
        root: root.clone(),
        context: context(&root)?,
    };
    if let Some((old, prior)) = existing(journal, nodes, identity, &recipe.enrollment.owner)? {
        if old != root || codec::encode(&prior)? != bytes {
            return Err("changed recipe under admitted operation".into());
        }
    } else {
        codec::put(nodes, &bytes)?;
    }
    nodes
        .finish()
        .map_err(|_| "cannot finish recipe metadata; effects may exist")?;
    if codec::get(nodes, &root)? != bytes {
        return Err("recipe metadata readback changed".into());
    }
    let prepare = command(identity, "prepare")?;
    let prepared = journal
        .apply(
            prepare,
            Command::Prepare {
                source: source(identity)?,
                owner: recipe.enrollment.owner.clone(),
                expected_revision: 0,
                selection,
            },
        )
        .map_err(|_| "recipe prepare pending; reconcile exact input")?;
    journal
        .apply(
            command(identity, "adopt")?,
            Command::Adopt {
                source: source(identity)?,
                owner: recipe.enrollment.owner.clone(),
                expected_revision: 1,
                prepared: prepared.reference,
            },
        )
        .map_err(|_| "recipe adoption pending; reconcile exact input")?;
    Ok(root)
}

pub fn load(
    journal: &mut SourceJournal,
    nodes: &MetadataStore,
    handle: &Handle,
) -> Result<Recipe, String> {
    if handle.authorities.journal.identity != *journal.identity() {
        return Err("foreign work journal".into());
    }
    let candidate: Recipe = codec::decode(&codec::get(nodes, &handle.recipe)?)?;
    let identity = candidate
        .controller_operation
        .as_ref()
        .unwrap_or(&handle.operation);
    let (root, recipe) = existing(journal, nodes, identity, &handle.enrollment.owner)?
        .ok_or("saved retirement recipe unavailable")?;
    let state = journal
        .source(&source(identity)?)
        .map_err(|_| "retirement recipe state unavailable")?
        .ok_or("saved retirement recipe missing")?;
    if recipe.policy_authority.is_some() != handle.policy_minimum.is_some()
        || recipe.operation != handle.operation
        || root != handle.recipe
        || state.revision != 2
        || state.pending.is_some()
        || recipe.enrollment.subject != handle.enrollment.subject
        || !recipe.authorities.same_binding(&handle.authorities)
    {
        return Err("work handle does not identify exact adopted recipe".into());
    }
    Ok(recipe)
}
