//! Exact bounded action plans acknowledged before native admission, no census.
use super::{codec, model::Recipe, session::Session};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tig_core::{Digest, Object};
use tig_store::{
    Key,
    source_claims::{Command, Guard, Selection},
};

const SCHEMA: &str = "tig.native-retirement-step.experimental-1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    recipe: Object,
    operation: Digest,
    input: Digest,
    work: Object,
}
fn key(root: &Object, input: &Digest) -> Result<Key, String> {
    Key::parse(&format!(
        "retire-step-{}",
        Digest::blob(&codec::encode(&(SCHEMA, root, input))?).as_str()
    ))
    .map_err(|_| "invalid native action key".into())
}
fn command(id: &Key, phase: &str) -> Result<Key, String> {
    Key::parse(&format!("{phase}-{}", id.as_str()))
        .map_err(|_| "invalid native action command".into())
}
fn context(root: &Object) -> Result<Digest, String> {
    Ok(Digest::blob(&codec::encode(&(SCHEMA, root))?))
}
pub fn load<T: DeserializeOwned>(
    session: &mut Session,
    recipe: &Recipe,
    root: &Object,
    input: &Digest,
) -> Result<Option<T>, String> {
    let id = key(root, input)?;
    let Some(state) = session
        .journal
        .source(&id)
        .map_err(|_| "native action authority unavailable")?
    else {
        return Ok(None);
    };
    if state.owner != recipe.enrollment.owner || state.cancelled || !state.predecessors.is_empty() {
        return Err("foreign/cancelled native action record".into());
    }
    let selection = match (&state.current, &state.pending) {
        (Some(current), None) if state.revision == 2 && current.revision == 2 => &current.selection,
        (None, Some(pending)) if state.revision == 1 => &pending.selection,
        _ => return Err("invalid native action record state".into()),
    };
    if context(&selection.root)? != selection.context {
        return Err("native action context changed".into());
    }
    let bytes = codec::get(&session.nodes, &selection.root)?;
    let record: Record = codec::decode(&bytes)?;
    if record.schema != SCHEMA
        || record.recipe != *root
        || record.operation != recipe.operation
        || record.input != *input
        || codec::encode(&record)? != bytes
    {
        return Err("native action binding/integrity mismatch".into());
    }
    Ok(Some(codec::decode(&codec::get(
        &session.nodes,
        &record.work,
    )?)?))
}
pub fn save<T: Serialize + DeserializeOwned>(
    session: &mut Session,
    recipe: &Recipe,
    root: &Object,
    input: &Digest,
    work: &T,
) -> Result<(), String> {
    let bytes = codec::encode(work)?;
    if let Some(prior) = load::<T>(session, recipe, root, input)?
        && codec::encode(&prior)? != bytes
    {
        return Err("changed plan under admitted action".into());
    }
    let work = codec::put(&mut session.nodes, &bytes)?;
    let record = Record {
        schema: SCHEMA.into(),
        recipe: root.clone(),
        operation: recipe.operation.clone(),
        input: input.clone(),
        work,
    };
    let record = codec::put(&mut session.nodes, &codec::encode(&record)?)?;
    session
        .nodes
        .finish()
        .map_err(|_| "native action metadata finish uncertain")?;
    let id = key(root, input)?;
    let prepared = session
        .journal
        .apply(
            command(&id, "prepare")?,
            Command::Prepare {
                source: id.clone(),
                owner: recipe.enrollment.owner.clone(),
                expected_revision: 0,
                selection: Selection {
                    context: context(&record)?,
                    root: record,
                },
            },
        )
        .map_err(|_| "native action prepare uncertain; retry exact work")?;
    session
        .journal
        .apply(
            command(&id, "adopt")?,
            Command::AdoptGuarded {
                source: id,
                owner: recipe.enrollment.owner.clone(),
                expected_revision: 1,
                prepared: prepared.reference,
                guards: vec![Guard {
                    source: Key::parse(recipe.operation.as_str())
                        .map_err(|_| "invalid parent intent")?,
                    owner: recipe.enrollment.owner.clone(),
                    expected_revision: 2,
                }],
            },
        )
        .map_err(|_| "native action adoption uncertain; retry exact work")?;
    Ok(())
}
