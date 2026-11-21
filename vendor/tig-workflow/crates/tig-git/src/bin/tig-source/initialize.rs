use super::{
    init_profile::Profile,
    profile::{Authority, Metadata},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use tig_core::{
    Digest,
    identity::canonical_json,
    tree::{Tree, TreeLimits},
};
use tig_git::source::{GitId, Limits, Repository, Scope, protection_work as observed};
use tig_store::{
    initialization::{self, Namespace},
    metadata::{self, MetadataStore},
    source_claims::{
        Selection, SourceJournal, coordinated,
        protection::{Index, Routing, work},
    },
};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    intent: Digest,
    metadata: Option<metadata::Checkpoint>,
    index: Option<Authority>,
    journal: Option<Authority>,
    selections: Option<BTreeMap<String, Selection>>,
    ready: bool,
}
fn encode(v: &impl Serialize) -> Result<Vec<u8>, String> {
    canonical_json(&serde_json::to_value(v).map_err(|_| "invalid initialization record")?)
        .map_err(|_| "source initialization record budget".into())
}
fn save(stage: &Path, record: &Record) -> Result<(), String> {
    let pending = stage.join("source-init.pending");
    if initialization::present(&pending)
        .map_err(|_| "cannot inspect initialization pending record")?
    {
        initialization::regular(&pending).map_err(|_| "foreign initialization pending record")?
    }
    let bytes = encode(record)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&pending)
        .map_err(|_| "cannot write initialization record")?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| "cannot persist initialization record")?;
    drop(f);
    fs::rename(pending, stage.join("source-init.json"))
        .map_err(|_| "cannot publish initialization record")?;
    Ok(())
}
fn check_stage(stage: &Path) -> Result<(), String> {
    if !initialization::present(stage).map_err(|_| "cannot inspect source stage")? {
        fs::create_dir(stage).map_err(|_| "cannot create source stage")?
    }
    initialization::directory(stage).map_err(|_| "foreign source stage")?;
    for (n, entry) in fs::read_dir(stage)
        .map_err(|_| "cannot list bounded source stage")?
        .enumerate()
    {
        if n >= 5 {
            return Err("unknown source stage entries".into());
        }
        let entry = entry.map_err(|_| "cannot inspect source stage entry")?;
        match entry.file_name().to_str() {
            Some("nodes" | "index" | "journal") => {
                initialization::directory(&entry.path()).map_err(|_| "foreign source component")?
            }
            Some("source-init.json" | "source-init.pending") => {
                initialization::regular(&entry.path()).map_err(|_| "foreign source record")?
            }
            _ => return Err("unknown source stage entry".into()),
        }
    }
    Ok(())
}
fn capture(p: &Profile, nodes: &mut MetadataStore) -> Result<BTreeMap<String, Selection>, String> {
    let repo = Repository::new(
        p.git_executable.clone(),
        p.repository.clone(),
        Limits::default(),
    )
    .map_err(|_| "cannot open initial Git source")?;
    let commit = GitId::parse(&p.commit).map_err(|_| "invalid initial commit")?;
    observed::validate_current(&repo, &p.binding, &commit)
        .map_err(|_| "initial source ref is not current")?;
    let extract = |id: &str,
                   scope: &[tig_core::LogicalPath],
                   nodes: &mut MetadataStore|
     -> Result<Selection, String> {
        let snapshot = repo
            .commit(GitId::parse(id).map_err(|_| "invalid initialization commit")?)
            .map_err(|_| "cannot read initialization commit")?;
        let e = repo
            .extract(
                &snapshot,
                Scope::new(scope.to_vec()).map_err(|_| "invalid initialization scope")?,
            )
            .map_err(|_| "cannot extract initialization selection")?;
        let tree = e
            .write_tree(nodes, TreeLimits::default())
            .map_err(|_| "incomplete/over-budget initialization selection")?;
        Ok(Selection {
            root: tree.root().clone(),
            context: e
                .selection_digest()
                .map_err(|_| "incomplete initialization context")?,
        })
    };
    let mut selections = BTreeMap::from([(
        p.binding.actor.as_str().into(),
        extract(&p.commit, &p.binding.scope, nodes)?,
    )]);
    for actor in &p.companions {
        let selected = if let Some(s) = &actor.selection {
            extract(&s.commit, &s.scope, nodes)?
        } else {
            Selection {
                root: Tree::empty(nodes)
                    .map_err(|_| "cannot create empty actor selection")?
                    .root()
                    .clone(),
                context: Digest::blob(b"tig.empty-source-initialization.experimental-1"),
            }
        };
        selections.insert(actor.actor.as_str().into(), selected);
    }
    observed::validate_current(&repo, &p.binding, &commit)
        .map_err(|_| "initial source ref changed during capture")?;
    nodes
        .finish()
        .map_err(|_| "cannot persist initial source metadata")?;
    Ok(selections)
}
fn expected_binding(
    p: &Profile,
    index: &Digest,
    selections: &BTreeMap<String, Selection>,
) -> Result<coordinated::Binding, String> {
    Ok(coordinated::Binding {
        index: index.clone(),
        source: p.source.clone(),
        actor: p.binding.actor.clone(),
        owner: p.binding.owner.clone(),
        initial: selections
            .get(p.binding.actor.as_str())
            .ok_or("initial main selection missing")?
            .clone(),
    })
}
fn published(p: &Profile, intent: &Digest) -> Result<Vec<u8>, String> {
    initialization::directory(&p.root).map_err(|_| "foreign source package")?;
    let record: Record = initialization::read_record(&p.root.join("source-init.json"))
        .map_err(|_| "source initialization receipt unavailable")?;
    if record.schema != "tig.source-initialization-record.experimental-1"
        || record.intent != *intent
        || !record.ready
    {
        return Err("source initialization identity/readiness mismatch".into());
    }
    let i = record.index.ok_or("index witness missing")?;
    let j = record.journal.ok_or("journal witness missing")?;
    let m = record.metadata.ok_or("metadata witness missing")?;
    if i.root != p.root.join("index") || j.root != p.root.join("journal") {
        return Err("source initialization roots changed".into());
    }
    let mut index =
        Index::open(&i.root, &i.minimum_checkpoint).map_err(|_| "cannot open initialized index")?;
    let journal = SourceJournal::open(&j.root, &j.minimum_checkpoint)
        .map_err(|_| "cannot open initialized journal")?;
    let mut nodes = MetadataStore::open(&p.root.join("nodes"), &m)
        .map_err(|_| "cannot open initialized metadata")?;
    let selections = record.selections.ok_or("initial selections missing")?;
    if index.identity() != &i.identity
        || journal.identity() != &j.identity
        || journal.coordinator() != Some(&expected_binding(p, &i.identity, &selections)?)
        || !index.requires_bound_source()
        || !index.requires_outcome_room()
    {
        return Err("initialized source bindings changed".into());
    }
    let mut actors = BTreeMap::new();
    for actor in &p.enrollment().actors {
        actors.insert(
            actor.id.as_str().to_string(),
            index
                .actor(&mut nodes, &actor.id)
                .map_err(|_| "initialized actor projection unavailable")?,
        );
    }
    let output = json!({"schema":"tig.source-enrollment.experimental-1","operation":p.operation,"intent":intent,"actors":actors,
        "git_executable":p.git_executable,"repository":p.repository,"binding":p.binding,"initial_commit":p.commit,
        "index":Authority {minimum_checkpoint:index.checkpoint(),..i},"journal":Authority {minimum_checkpoint:journal.checkpoint(),..j},
        "metadata":Metadata {root:p.root.join("nodes"),minimum_checkpoint:nodes.checkpoint()},
        "placement_executed":false,"custody":"unknown","removal_authorized":false});
    encode(&output)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    ProgressReady,
    MetadataReady,
    SelectionsReady,
    IndexReady,
    IndexBootstrapped,
    JournalReady,
    Enrolled,
    Published,
}
pub fn initialize(p: Profile) -> Result<Vec<u8>, String> {
    initialize_hook(p, |_| Ok(()))
}
fn initialize_hook(
    mut p: Profile,
    mut hook: impl FnMut(Stage) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    p.root = initialization::canonical_root(&p.root)
        .map_err(|_| "invalid source package root/parent")?;
    let intent = Digest::blob(&encode(&json!([
        "tig.source-initialization-intent.experimental-1",
        &p
    ]))?);
    let namespace = Namespace::new(&p.root, intent.clone())
        .map_err(|_| "invalid source initialization namespace")?;
    if initialization::present(&p.root).map_err(|_| "cannot inspect source package")? {
        return published(&p, &intent);
    }
    let captured = if initialization::present(namespace.stage())
        .map_err(|_| "cannot inspect source stage")?
    {
        initialization::directory(namespace.stage()).map_err(|_| "foreign source stage")?;
        let frame = namespace.stage().join("source-init.json");
        if initialization::present(&frame).map_err(|_| "cannot inspect captured source state")? {
            let r: Record = initialization::read_record(&frame)
                .map_err(|_| "partial source progress; preserve and reconcile")?;
            if r.intent != intent {
                return Err("source initialization capture conflicts".into());
            }
            r.selections.is_some()
        } else {
            false
        }
    } else {
        false
    };
    if !captured {
        let repo = Repository::new(
            p.git_executable.clone(),
            p.repository.clone(),
            Limits::default(),
        )
        .map_err(|_| "cannot open initial Git source")?;
        observed::validate_current(
            &repo,
            &p.binding,
            &GitId::parse(&p.commit).map_err(|_| "invalid initial commit")?,
        )
        .map_err(|_| "initial source ref is not current")?;
    }
    let _fence = namespace
        .fence()
        .map_err(|e| format!("source initialization {e}; retry or reconcile exact input"))?;
    if initialization::present(&p.root).map_err(|_| "cannot recheck source package")? {
        return published(&p, &intent);
    }
    check_stage(namespace.stage())?;
    let path = namespace.stage().join("source-init.json");
    let mut record =
        if initialization::present(&path).map_err(|_| "cannot inspect source progress")? {
            initialization::read_record::<Record>(&path)
                .map_err(|_| "partial source progress; preserve and reconcile")?
        } else {
            Record {
                schema: "tig.source-initialization-record.experimental-1".into(),
                intent: intent.clone(),
                metadata: None,
                index: None,
                journal: None,
                selections: None,
                ready: false,
            }
        };
    if record.schema != "tig.source-initialization-record.experimental-1" || record.intent != intent
    {
        return Err("source initialization progress conflict".into());
    }
    save(namespace.stage(), &record)?;
    hook(Stage::ProgressReady)?;
    let nodes_path = namespace.stage().join("nodes");
    let mut nodes = if let Some(minimum) = &record.metadata {
        MetadataStore::open(&nodes_path, minimum)
            .map_err(|_| "cannot resume witnessed source metadata")?
    } else {
        MetadataStore::create(&nodes_path, p.metadata_limits)
            .map_err(|_| "source metadata creation pending; preserve partial component")?
    };
    record.metadata = Some(nodes.checkpoint());
    save(namespace.stage(), &record)?;
    hook(Stage::MetadataReady)?;
    if record.selections.is_none() {
        record.selections = Some(capture(&p, &mut nodes)?);
        record.metadata = Some(nodes.checkpoint());
        save(namespace.stage(), &record)?;
    }
    let selections = record
        .selections
        .as_ref()
        .ok_or("initial selections missing")?
        .clone();
    hook(Stage::SelectionsReady)?;
    let mut index = if let Some(i) = &record.index {
        let opened = Index::open(&namespace.stage().join("index"), &i.minimum_checkpoint)
            .map_err(|_| "cannot resume initialized index")?;
        if opened.identity() != &i.identity {
            return Err("source index identity changed".into());
        }
        opened
    } else {
        Index::create_recoverable(
            &namespace.stage().join("index"),
            p.index_epoch.clone(),
            p.enrollment(),
            Routing {
                source: p.source.clone(),
                actor: p.binding.actor.clone(),
            },
        )
        .map_err(|_| "source index creation pending; preserve partial component")?
    };
    record.index = Some(Authority {
        root: p.root.join("index"),
        identity: index.identity().clone(),
        minimum_checkpoint: index.checkpoint(),
    });
    save(namespace.stage(), &record)?;
    hook(Stage::IndexReady)?;
    index
        .bootstrap_recoverable(selections.clone(), &mut nodes)
        .map_err(|_| "source index bootstrap pending; retry exact input")?;
    record.metadata = Some(nodes.checkpoint());
    record.index.as_mut().unwrap().minimum_checkpoint = index.checkpoint();
    save(namespace.stage(), &record)?;
    hook(Stage::IndexBootstrapped)?;
    let binding = expected_binding(&p, index.identity(), &selections)?;
    let mut journal = if let Some(j) = &record.journal {
        let opened = SourceJournal::open(&namespace.stage().join("journal"), &j.minimum_checkpoint)
            .map_err(|_| "cannot resume initialized source journal")?;
        if opened.identity() != &j.identity || opened.coordinator() != Some(&binding) {
            return Err("source journal identity/binding changed".into());
        }
        opened
    } else {
        SourceJournal::create_coordinated(
            &namespace.stage().join("journal"),
            p.project.clone(),
            p.journal_epoch.clone(),
            binding,
        )
        .map_err(|_| "source journal creation pending; preserve partial component")?
    };
    record.journal = Some(Authority {
        root: p.root.join("journal"),
        identity: journal.identity().clone(),
        minimum_checkpoint: journal.checkpoint(),
    });
    save(namespace.stage(), &record)?;
    hook(Stage::JournalReady)?;
    work::adoption::enrollment::initialize(&mut index, &mut journal, &mut nodes)
        .map_err(|_| "source identity enrollment pending; retry exact input")?;
    record.metadata = Some(nodes.checkpoint());
    record.index.as_mut().unwrap().minimum_checkpoint = index.checkpoint();
    record.journal.as_mut().unwrap().minimum_checkpoint = journal.checkpoint();
    record.ready = true;
    save(namespace.stage(), &record)?;
    hook(Stage::Enrolled)?;
    drop(index);
    drop(journal);
    drop(nodes);
    namespace
        .publish()
        .map_err(|_| "source package publication pending; retry exact input")?;
    hook(Stage::Published)?;
    published(&p, &intent)
}

#[cfg(test)]
#[path = "initialize/tests.rs"]
mod tests;
