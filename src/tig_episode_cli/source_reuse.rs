//! Explicit reuse of this producer's completely retired component identities.
//! This changes no journal guard and grants no media or approval authority.
use super::*;
use std::collections::BTreeSet;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Child {
    pub source: String,
    pub revision: u64,
}
pub(super) type Pool = BTreeMap<String, Child>;

pub(super) fn labels(inputs: &Inputs) -> Vec<String> {
    inputs
        .scenes
        .iter()
        .map(|s| format!("scene/{}", s.id))
        .chain(
            inputs
                .presentations
                .iter()
                .map(|p| format!("presentation/{}", p.id)),
        )
        .chain(std::iter::once("parent".to_owned()))
        .collect()
}

pub(super) fn resolve(p: &Profile) -> Result<Option<Pool>> {
    let Some(selection) = &p.reuse_children else {
        return Ok(None);
    };
    let wait = MetadataWait::default();
    let mut reader = Waiting::new(
        wait.run(|| MetadataReader::open(&p.metadata_root, &p.metadata_minimum))?,
        &wait,
    );
    let header = handoff::retained_header(selection, &mut reader)?;
    pool_from_header(
        &labels(&p.inputs),
        &p.source_id,
        &p.owner,
        p.expected_revision,
        &header,
    )
    .map(Some)
}

fn pool_from_header(
    labels: &[String],
    source: &str,
    owner: &str,
    revision: u64,
    header: &Value,
) -> Result<Pool> {
    if header["schema"] != "reel.tig-episode-retained-context.experimental-1"
        || header["source_id"] != source
        || header["owner"] != owner
        || header["root_revision_after"].as_u64() != Some(revision)
        || !header["succeeded"].is_boolean()
        || header["approval"] != false
        || labels.iter().collect::<BTreeSet<_>>().len() != labels.len()
    {
        bail!("episode reuse completed root identity differs");
    }
    let temporary = header["temporary"]
        .as_array()
        .context("episode reuse temporary sources missing")?;
    let expected: Vec<String> = serde_json::from_value(header["expected_temporary"].clone())?;
    if temporary.len() > labels.len()
        || expected.len() != labels.len()
        || expected.iter().collect::<BTreeSet<_>>().len() != expected.len()
    {
        bail!("episode reuse requires a complete component pool");
    }
    let previous: Option<Pool> = header
        .get("reused_children")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?;
    let succeeded = header["succeeded"]
        .as_bool()
        .context("episode reuse outcome missing")?;
    if temporary.len() != labels.len() && (succeeded || previous.is_none()) {
        bail!("episode reuse requires a complete established component pool");
    }
    let temporary_ids = temporary
        .iter()
        .map(|entry| {
            entry["source"]
                .as_str()
                .context("episode reuse source missing")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if temporary_ids.len() != temporary.len()
        || temporary_ids
            .iter()
            .any(|id| !expected.iter().any(|expected| expected == id))
    {
        bail!("episode reuse temporary scope differs");
    }
    if previous.as_ref().is_some_and(|pool| {
        pool.len() != labels.len() || labels.iter().any(|label| !pool.contains_key(label))
    }) {
        bail!("episode reuse component labels differ");
    }
    let operation = header["operation"]
        .as_str()
        .context("episode reuse prior operation missing")?;
    let mut pool = Pool::new();
    let mut ids = BTreeSet::new();
    for label in labels {
        let (id, before) = match &previous {
            Some(previous) => {
                let child = &previous[label];
                (child.source.clone(), child.revision)
            }
            None => (phase(source, operation, label)?.as_str().to_owned(), 0),
        };
        key(&id)?;
        if id == source || !ids.insert(id.clone()) || !expected.contains(&id) {
            bail!("episode reuse source identity aliases or differs");
        }
        let matching = temporary
            .iter()
            .filter(|entry| entry["source"] == id)
            .collect::<Vec<_>>();
        if matching.is_empty() && !succeeded && previous.is_some() {
            pool.insert(
                label.clone(),
                Child {
                    source: id,
                    revision: before,
                },
            );
            continue;
        }
        if matching.len() != 1 {
            bail!("episode reuse source retirement missing or duplicated");
        }
        let old = matching[0];
        let selected: Selection = serde_json::from_value(old["selection"].clone())?;
        if selected.root.bytes == 0 || selected.root.bytes > 65_536 {
            bail!("episode reuse prior selection differs");
        }
        let prepared: Option<source_claims::Reference> = old
            .get("prepared")
            .filter(|value| !value.is_null())
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()?;
        let pending = prepared.is_some();
        if succeeded && pending {
            bail!("episode successful handoff contains an unadopted component");
        }
        let active_revision = before
            .checked_add(if pending { 1 } else { 2 })
            .context("episode reuse revision overflow")?;
        if old["revision"].as_u64() != Some(active_revision) {
            bail!("episode reuse prior source revision differs");
        }
        let retired = active_revision
            .checked_add(1)
            .context("episode reuse retired revision overflow")?;
        retired
            .checked_add(3)
            .context("episode reuse next revision overflow")?;
        pool.insert(
            label.clone(),
            Child {
                source: id,
                revision: retired,
            },
        );
    }
    validate_pool(labels, source, Some(&pool))?;
    Ok(pool)
}

pub(super) fn validate_pool(labels: &[String], root: &str, pool: Option<&Pool>) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    let mut ids = BTreeSet::new();
    if pool.len() != labels.len() || labels.iter().any(|label| !pool.contains_key(label)) {
        bail!("episode reuse pool labels differ");
    }
    for child in pool.values() {
        key(&child.source)?;
        if child.source == root || !ids.insert(&child.source) || child.revision == 0 {
            bail!("episode reuse pool source aliases or is unretired");
        }
        child
            .revision
            .checked_add(3)
            .context("episode reuse revision overflow")?;
    }
    Ok(())
}

pub(super) fn source(
    source: &str,
    operation: &str,
    label: &str,
    pool: Option<&Pool>,
) -> Result<Key> {
    let id = match pool {
        Some(pool) => key(&pool
            .get(label)
            .context("episode reuse label missing")?
            .source)?,
        None => phase(source, operation, label)?,
    };
    if id == key(source)? {
        bail!("episode child cannot alias runner source");
    }
    Ok(id)
}

pub(super) fn revision(label: &str, pool: Option<&Pool>) -> Result<u64> {
    match pool {
        Some(pool) => Ok(pool
            .get(label)
            .context("episode reuse label missing")?
            .revision),
        None => Ok(0),
    }
}

pub(super) fn expected(p: &Profile, pool: Option<&Pool>) -> Result<Vec<String>> {
    labels(&p.inputs)
        .iter()
        .map(|label| {
            Ok(source(&p.source_id, &p.operation, label, pool)?
                .as_str()
                .to_owned())
        })
        .collect()
}

pub(super) fn check_retired(
    p: &Profile,
    pool: Option<&Pool>,
    journal: &mut SourceJournal,
) -> Result<()> {
    let Some(pool) = pool else { return Ok(()) };
    check_states(
        &p.source_id,
        &p.owner,
        p.expected_revision,
        p.reuse_children
            .as_ref()
            .context("episode reuse selection missing")?,
        pool,
        journal,
    )
}

fn check_states(
    source: &str,
    owner: &str,
    revision: u64,
    selection: &Selection,
    pool: &Pool,
    journal: &mut SourceJournal,
) -> Result<()> {
    let root = journal
        .source(&key(source)?)?
        .context("episode reuse root missing")?;
    if root.owner != key(owner)?
        || root.revision != revision
        || root.pending.is_some()
        || root.current.as_ref().map(|current| &current.selection) != Some(selection)
        || root.predecessors.len() + 3 > source_claims::MAX_PREDECESSORS
    {
        bail!("episode reuse root is stale, foreign, active or history-bound");
    }
    for child in pool.values() {
        let state = journal
            .source(&key(&child.source)?)?
            .context("episode reuse child missing")?;
        if state.owner != key(owner)?
            || state.revision != child.revision
            || state.current.is_some()
            || state.pending.is_some()
            || state.predecessors.len() + 1 > source_claims::MAX_PREDECESSORS
        {
            bail!("episode reuse child is stale, foreign, active or history-bound");
        }
    }
    Ok(())
}
