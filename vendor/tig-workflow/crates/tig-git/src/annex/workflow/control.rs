//! Explicit control metadata bootstrap. Does not initialize or scan payload stores.
use super::{invalid, retirement::MetadataBinding};
use crate::source::{GitId, Limits, Repository, Scope};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::PathBuf,
};
use tig_core::{
    Digest, LogicalPath, Object,
    tree::{Tree, TreeLimits},
};
use tig_store::{
    Key,
    metadata::{self, MetadataStore},
    source_claims::{
        self, Selection, SourceJournal,
        protection::{Enrollment, Index, Update},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Seed {
    Empty,
    Git {
        executable: PathBuf,
        repository: PathBuf,
        commit: String,
        scope: Vec<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Init {
    pub index_root: PathBuf,
    pub metadata_root: PathBuf,
    pub intent_root: PathBuf,
    pub epoch: Key,
    pub enrollment: Enrollment,
    pub seeds: BTreeMap<String, Seed>,
    pub metadata_limits: metadata::Limits,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub index_root: PathBuf,
    pub index: Digest,
    pub index_minimum: source_claims::Checkpoint,
    pub metadata: MetadataBinding,
    pub intent_root: PathBuf,
    pub intent: Digest,
    pub intent_minimum: source_claims::Checkpoint,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adoption {
    pub package: Package,
    pub actor: Key,
    pub owner: Key,
    pub operation: Key,
    pub expected_revision: u64,
    pub seed: Seed,
    pub changes: BTreeMap<LogicalPath, Option<Object>>,
    pub ready: bool,
}
fn extract(seed: &Seed) -> io::Result<Option<crate::source::Extraction>> {
    let Seed::Git {
        executable,
        repository,
        commit,
        scope,
    } = seed
    else {
        return Ok(None);
    };
    let reader = Repository::new(executable.clone(), repository.clone(), Limits::default())
        .map_err(|_| invalid())?;
    let snapshot = reader
        .commit(GitId::parse(commit).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let scope = Scope::new(
        scope
            .iter()
            .map(|s| LogicalPath::parse(s))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    let extracted = reader
        .extract_annex(&snapshot, scope)
        .map_err(|_| invalid())?;
    if !extracted.complete() {
        return Err(invalid());
    }
    Ok(Some(extracted))
}
fn selection(
    actor: &Key,
    extracted: Option<&crate::source::Extraction>,
    nodes: &mut MetadataStore,
) -> io::Result<Selection> {
    if let Some(extracted) = extracted {
        Ok(Selection {
            root: extracted
                .write_tree(nodes, TreeLimits::default())
                .map_err(|_| invalid())?
                .root()
                .clone(),
            context: extracted.selection_digest().map_err(|_| invalid())?,
        })
    } else {
        Ok(Selection {
            root: Tree::empty(nodes).map_err(|_| invalid())?.root().clone(),
            context: Digest::blob(
                format!("tig.annex-empty-source.design-1\n{}", actor.as_str()).as_bytes(),
            ),
        })
    }
}
/// Caller supplies a bounded exact delta. The original index recomputes it from
/// the old actor root and requires equality to the complete extracted next root.
/// This is explicit local admission, not authentication of a moving hosted ref.
pub fn adopt(input: &Adoption) -> io::Result<Package> {
    if input.changes.len() > tig_store::source_claims::protection::MAX_CHANGES {
        return Err(invalid());
    }
    let mut index = Index::open(&input.package.index_root, &input.package.index_minimum)
        .map_err(|_| invalid())?;
    if index.identity() != &input.package.index || !index.supports_effect_fences() {
        return Err(invalid());
    }
    let extracted = extract(&input.seed)?;
    let mut nodes = MetadataStore::open(
        &input.package.metadata.root,
        &input.package.metadata.minimum,
    )
    .map_err(|_| invalid())?;
    let selected = selection(&input.actor, extracted.as_ref(), &mut nodes)?;
    index
        .update(
            &Update {
                operation: input.operation.clone(),
                actor: input.actor.clone(),
                owner: input.owner.clone(),
                expected_revision: input.expected_revision,
                selection: selected,
                changes: input.changes.clone(),
                ready: input.ready,
            },
            &mut nodes,
        )
        .map_err(|_| invalid())?;
    let mut package = input.package.clone();
    package.index_minimum = index.checkpoint();
    package.metadata.minimum = nodes.checkpoint();
    Ok(package)
}
pub fn initialize(profile: &Init) -> io::Result<Package> {
    let roots = [
        &profile.index_root,
        &profile.metadata_root,
        &profile.intent_root,
    ];
    if roots.iter().any(|p| !p.is_absolute() || p.exists())
        || roots.iter().collect::<BTreeSet<_>>().len() != 3
    {
        return Err(invalid());
    }
    profile.enrollment.validate().map_err(|_| invalid())?;
    if profile.seeds.keys().map(|s| s.as_str()).collect::<Vec<_>>()
        != profile
            .enrollment
            .actors
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>()
    {
        return Err(invalid());
    }
    // Finish exact read-only extraction before creating any control roots.
    let mut extractions = BTreeMap::new();
    for (actor, seed) in &profile.seeds {
        if let Some(extracted) = extract(seed)? {
            extractions.insert(actor.clone(), extracted);
        }
    }
    let mut nodes = MetadataStore::create(&profile.metadata_root, profile.metadata_limits)
        .map_err(|_| invalid())?;
    let mut selections = BTreeMap::new();
    for actor in &profile.enrollment.actors {
        let selected = selection(&actor.id, extractions.get(actor.id.as_str()), &mut nodes)?;
        selections.insert(actor.id.as_str().into(), selected);
    }
    nodes.finish().map_err(|_| invalid())?;
    let mut index = Index::create_effects(
        &profile.index_root,
        profile.epoch.clone(),
        profile.enrollment.clone(),
        None,
    )
    .map_err(|_| invalid())?;
    index
        .bootstrap(selections, &mut nodes)
        .map_err(|_| invalid())?;
    let intent = SourceJournal::create(
        &profile.intent_root,
        Key::parse("annex-intents").map_err(|_| invalid())?,
        profile.epoch.clone(),
    )
    .map_err(|_| invalid())?;
    Ok(Package {
        index_root: profile.index_root.clone(),
        index: index.identity().clone(),
        index_minimum: index.checkpoint(),
        metadata: MetadataBinding {
            root: profile.metadata_root.clone(),
            minimum: nodes.checkpoint(),
        },
        intent_root: profile.intent_root.clone(),
        intent: intent.identity().clone(),
        intent_minimum: intent.checkpoint(),
    })
}
