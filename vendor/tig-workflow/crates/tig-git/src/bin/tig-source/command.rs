use super::profile::{self, Authority, Metadata, Profile};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};
use tig_core::{
    Digest, LogicalPath, Object,
    identity::{canonical_json, parse_json},
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_git::source::{
    GitId, Limits, Repository, Scope,
    protection_work::{self as observed, Binding, Event},
};
use tig_store::{
    Key,
    metadata::MetadataStore,
    source_claims::{
        Selection, SourceJournal,
        protection::{
            self, Index, Update,
            work::{self, WorkRef},
        },
    },
};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    schema: String,
    operation: Key,
    git_executable: PathBuf,
    repository: PathBuf,
    binding: Binding,
    commit: String,
    index: Authority,
    journal: Authority,
    metadata: Metadata,
    reference: WorkRef,
    digest: Digest,
}
fn fingerprint(w: &Work) -> Result<Digest, String> {
    let mut value = serde_json::to_value(w).map_err(|_| "invalid source work")?;
    value
        .as_object_mut()
        .ok_or("invalid source work")?
        .remove("digest");
    Ok(Digest::blob(
        &canonical_json(&value).map_err(|_| "source work budget")?,
    ))
}
pub fn parse_work(bytes: &[u8]) -> Result<Work, String> {
    let w: Work = serde_json::from_value(
        parse_json(bytes).map_err(|_| "invalid/over-budget source work JSON")?,
    )
    .map_err(|_| "invalid source work fields")?;
    if w.schema != "tig.source-work.experimental-1" || fingerprint(&w)? != w.digest {
        return Err("source work identity mismatch".into());
    }
    profile::bindings(
        &w.git_executable,
        &w.repository,
        &w.index,
        &w.journal,
        &w.metadata,
    )?;
    profile::scope(&w.binding.scope)?;
    GitId::parse(&w.commit).map_err(|_| "exact work commit required")?;
    Ok(w)
}
fn open(
    index: &Authority,
    journal: &Authority,
    metadata: &Metadata,
) -> Result<(Index, SourceJournal, MetadataStore), String> {
    let i = Index::open(&index.root, &index.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed source index")?;
    let j = SourceJournal::open(&journal.root, &journal.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed source journal")?;
    let m = MetadataStore::open(&metadata.root, &metadata.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed source metadata")?;
    if i.identity() != &index.identity
        || j.identity() != &journal.identity
        || !i.requires_bound_source()
        || !i.requires_outcome_room()
        || i.routing().is_none()
    {
        return Err("source identity or recoverable routing profile mismatch".into());
    }
    Ok((i, j, m))
}
fn selection(
    repo: &Repository,
    commit: &str,
    scope: &[LogicalPath],
    nodes: &mut MetadataStore,
) -> Result<Selection, String> {
    let snapshot = repo
        .commit(GitId::parse(commit).map_err(|_| "exact projection commit required")?)
        .map_err(|_| "cannot inspect exact projection commit")?;
    let extraction = repo
        .extract(
            &snapshot,
            Scope::new(scope.to_vec()).map_err(|_| "invalid projection scope")?,
        )
        .map_err(|_| "cannot extract exact projection")?;
    let tree = extraction
        .write_tree(nodes, TreeLimits::default())
        .map_err(|_| "incomplete/over-budget source projection")?;
    Ok(Selection {
        root: tree.root().clone(),
        context: extraction
            .selection_digest()
            .map_err(|_| "incomplete projection context")?,
    })
}
fn changes(
    old: &Selection,
    new: &Selection,
    nodes: &mut MetadataStore,
) -> Result<BTreeMap<LogicalPath, Option<Object>>, String> {
    let (old, _) = Tree::from_root(old.root.clone())
        .entries(nodes, TreeLimits::default())
        .map_err(|_| "cannot inspect old actor projection")?;
    let (new, _) = Tree::from_root(new.root.clone())
        .entries(nodes, TreeLimits::default())
        .map_err(|_| "cannot inspect new actor projection")?;
    let mut result = BTreeMap::new();
    for (path, object) in &new {
        if old.get(path) != Some(object) {
            result.insert(path.clone(), Some(object.clone()));
        }
    }
    for path in old.keys() {
        if !new.contains_key(path) {
            result.insert(path.clone(), None);
        }
    }
    if result.len() > protection::MAX_CHANGES {
        return Err("source transition exceeds per-actor change budget".into());
    }
    Ok(result)
}
struct Target<'a> {
    operation: &'a Key,
    actor: &'a Key,
    owner: &'a Key,
    revision: u64,
    selection: Selection,
}
fn update(
    index: &mut Index,
    nodes: &mut MetadataStore,
    prior: Option<&work::Record>,
    target: Target<'_>,
) -> Result<Update, String> {
    let Target {
        operation,
        actor,
        owner,
        revision,
        selection,
    } = target;
    let delta = if let Some(prior) = prior {
        let saved = prior
            .updates()
            .find(|u| u.actor == *actor)
            .ok_or("staged actor set changed")?;
        if saved.operation != *operation
            || saved.owner != *owner
            || saved.expected_revision != revision
            || saved.selection != selection
            || !saved.ready
        {
            return Err("staged source projection changed".into());
        }
        saved.changes.clone()
    } else {
        let old = index
            .actor(nodes, actor)
            .map_err(|_| "cannot inspect enrolled source actor")?;
        if old.revision != revision || !old.ready {
            return Err("source actor revision is not current and ready".into());
        }
        changes(&old.selection, &selection, nodes)?
    };
    Ok(Update {
        operation: operation.clone(),
        actor: actor.clone(),
        owner: owner.clone(),
        expected_revision: revision,
        selection,
        changes: delta,
        ready: true,
    })
}
pub fn stage(p: Profile) -> Result<Vec<u8>, String> {
    let (mut index, mut journal, mut nodes) = open(&p.index, &p.journal, &p.metadata)?;
    let repository = Repository::new(
        p.git_executable.clone(),
        p.repository.clone(),
        Limits::default(),
    )
    .map_err(|_| "cannot open explicit Git source")?;
    let prior = work::lookup(&mut index, &p.operation)
        .map_err(|_| "cannot inspect prior source operation")?
        .map(|r| work::read(&mut index, &r, &nodes).map_err(|_| "cannot read prior source work"))
        .transpose()?;
    if prior
        .as_ref()
        .is_some_and(|r| r.updates().count() != p.companions.len() + 1)
    {
        return Err("staged source actor set changed".into());
    }
    let current = selection(&repository, &p.commit, &p.binding.scope, &mut nodes)?;
    let main = update(
        &mut index,
        &mut nodes,
        prior.as_ref(),
        Target {
            operation: &p.operation,
            actor: &p.binding.actor,
            owner: &p.binding.owner,
            revision: p.expected_revision,
            selection: current,
        },
    )?;
    let mut additional = Vec::new();
    for c in &p.companions {
        let target = selection(&repository, &c.commit, &c.scope, &mut nodes)?;
        additional.push(update(
            &mut index,
            &mut nodes,
            prior.as_ref(),
            Target {
                operation: &p.operation,
                actor: &c.actor,
                owner: &c.owner,
                revision: c.expected_revision,
                selection: target,
            },
        )?);
    }
    let event = Event {
        operation: p.operation.clone(),
        commit: GitId::parse(&p.commit).map_err(|_| "invalid source commit")?,
        expected_revision: p.expected_revision,
        changes: main.changes,
    };
    let reference = observed::stage_coordinated(
        &repository,
        &p.binding,
        &mut index,
        &event,
        &additional,
        &mut journal,
        &mut nodes,
    )
    .map_err(|e| {
        format!("source staging failed ({e}); metadata/work may exist; retry exact input")
    })?;
    let mut out = Work {
        schema: "tig.source-work.experimental-1".into(),
        operation: p.operation,
        git_executable: p.git_executable,
        repository: p.repository,
        binding: p.binding,
        commit: p.commit,
        index: p.index,
        journal: p.journal,
        metadata: p.metadata,
        reference,
        digest: Digest::blob(b"unsealed source work"),
    };
    out.index.minimum_checkpoint = index.checkpoint();
    out.journal.minimum_checkpoint = journal.checkpoint();
    out.metadata.minimum_checkpoint = nodes.checkpoint();
    out.digest = fingerprint(&out)?;
    canonical_json(
        &serde_json::to_value(out)
            .map_err(|_| "cannot encode staged source work; effects may exist")?,
    )
    .map_err(|_| "cannot encode staged source work; effects may exist".into())
}
fn check_record(index: &mut Index, nodes: &MetadataStore, w: &Work) -> Result<(), String> {
    let record = work::read(index, &w.reference, nodes)
        .map_err(|_| "cannot validate exact native source work")?;
    if record.adoption.is_none() || record.update.operation != w.operation {
        return Err("unlinked/foreign source work".into());
    }
    let evidence = record.evidence.ok_or("source work observation missing")?;
    let bytes = nodes
        .get(&evidence)
        .map_err(|_| "source work observation unavailable")?
        .ok_or("source observation missing")?;
    let observation = parse_json(&bytes).map_err(|_| "invalid source work observation")?;
    if observation["schema"] != "tig.git-protection-work.design-1"
        || observation["commit"] != w.commit
        || observation["binding"]
            != serde_json::to_value(&w.binding).map_err(|_| "invalid source binding")?
    {
        return Err("source work observation/binding changed".into());
    }
    Ok(())
}
pub fn apply(w: Work) -> Result<(Vec<u8>, u8), String> {
    let (mut index, mut journal, mut nodes) = open(&w.index, &w.journal, &w.metadata)?;
    check_record(&mut index, &nodes, &w)?;
    let result =
        match work::adoption::resume_admitted(&mut index, &mut journal, &w.reference, &mut nodes) {
            Err(protection::Error::Pending) => Repository::new(
                w.git_executable.clone(),
                w.repository.clone(),
                Limits::default(),
            )
            .map_err(observed::Error::Git)
            .and_then(|repo| {
                observed::execute_coordinated(
                    &repo,
                    &w.binding,
                    &mut index,
                    &mut journal,
                    &w.reference,
                    &mut nodes,
                )
            }),
            other => other.map_err(observed::Error::Index),
        };
    let (receipt, pending) = match result {
        Ok(r) => (
            Some(json!({"reference":r.reference,"checkpoint":r.checkpoint,
            "source_revision":r.source_revision,"replayed":r.replayed})),
            None,
        ),
        Err(e) => (
            None,
            Some(format!(
                "source adoption pending ({e}); effects may exist; resume exact work"
            )),
        ),
    };
    let complete = receipt.is_some();
    let out = json!({"schema":"tig.source-result.experimental-1","work_digest":w.digest,"operation":w.operation,"reference":w.reference,
        "source_adoption_complete":complete,"receipt":receipt,"pending":pending,"last_confirmed_minimum_checkpoints":{
            "index":index.checkpoint(),"journal":journal.checkpoint(),"metadata":nodes.checkpoint()},
        "placement_executed":false,"custody":"unknown","removal_authorized":false});
    let bytes =
        canonical_json(&out).map_err(|_| "cannot encode source outcome; effects may exist")?;
    Ok((bytes, if complete { 0 } else { 3 }))
}
