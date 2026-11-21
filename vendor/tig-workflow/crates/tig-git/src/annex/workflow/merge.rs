//! Bounded cooperative local merge runner. Original metadata/journal engines;
//! annex owns bytes, registered policy owns new eviction authority.
use super::{control::Package, invalid, policy, retirement};
use crate::annex::{
    identity::Key as AnnexKey,
    process::{ControlCommand, Enrollment, Remote, Runner},
    worker::Controller,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io,
    path::PathBuf,
};
use tig_core::{
    Digest, LogicalPath, Object,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::{
    Key,
    metadata::MetadataStore,
    source_claims::{
        self, Checkpoint, Command, Selection, SourceJournal,
        progress_budget::{self, BudgetRef},
        protection::{Index, Update, batch::Batch},
    },
};
mod catchup;
pub mod maintenance;
mod model;
mod plan;
mod staging;
mod store;
pub mod wake;
use model::*;
pub use model::{Config, Destination, Handle, Init, Limits, Mode, Report, Request, Status};
use store::*;
const SCHEMA: &str = "tig.annex-merge.experimental-3";
fn encode(v: &impl Serialize) -> io::Result<Vec<u8>> {
    tig_core::identity::canonical_json(&serde_json::to_value(v).map_err(|_| invalid())?)
        .map_err(|_| invalid())
}
fn k(s: &str) -> io::Result<Key> {
    Key::parse(s).map_err(|_| invalid())
}
fn key(role: &str, v: &impl Serialize) -> io::Result<Key> {
    k(&format!(
        "merge-{role}-{}",
        Digest::blob(&encode(&(SCHEMA, v))?).as_str()
    ))
}
fn run_key(op: &Digest) -> io::Result<Key> {
    key("run", op)
}
fn stronger(a: &Checkpoint, b: &Checkpoint) -> io::Result<Checkpoint> {
    if a.sequence() == b.sequence() && a != b {
        return Err(invalid());
    }
    Ok(if a.sequence() >= b.sequence() {
        a.clone()
    } else {
        b.clone()
    })
}
fn tree_limits() -> TreeLimits {
    TreeLimits {
        max_entries: 128,
        max_reads: 4096,
        max_writes: 4096,
        max_read_bytes: 1048576,
        max_write_bytes: 1048576,
        max_map_depth: 64,
    }
}
fn entries(nodes: &mut MetadataStore, root: &Object) -> io::Result<BTreeMap<LogicalPath, Object>> {
    Tree::from_root(root.clone())
        .entries(nodes, tree_limits())
        .map(|r| r.0)
        .map_err(|_| invalid())
}
fn retired(o: &Object) -> io::Result<LogicalPath> {
    LogicalPath::parse(&format!("retired/{}", o.sha256.as_str())).map_err(|_| invalid())
}
fn locked_file(path: &std::path::Path) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.share_mode(1).custom_flags(0x00200000);
    }
    let f = o.open(path)?;
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(invalid());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
    }
    Ok(f)
}
struct RefHold {
    _directories: Vec<File>,
    _reference: File,
    _config: File,
}
fn hold_ref(c: &Config, commit: &str) -> io::Result<RefHold> {
    plan::current(c, commit)?;
    let git = if c.caller.repository.join(".git").exists() {
        c.caller.repository.join(".git")
    } else {
        c.caller.repository.clone()
    };
    let path = git.join(&c.reference);
    let dirs = crate::annex::legacy::hold_directories(path.parent().ok_or_else(invalid)?)?;
    let reference = locked_file(&path)?;
    let config = locked_file(&git.join("config"))?;
    if reference.metadata()?.len() > 66 {
        return Err(invalid());
    }
    let text = fs::read_to_string(&path)?;
    if text.trim_end_matches(['\r', '\n']) != commit {
        return Err(invalid());
    }
    plan::current(c, commit)?;
    Ok(RefHold {
        _directories: dirs,
        _reference: reference,
        _config: config,
    })
}
fn root_lock(root: &std::path::Path) -> io::Result<(Vec<File>, File)> {
    let dirs = crate::annex::legacy::hold_directories(root)?;
    let path = root.join("controller.lock");
    let mut o = OpenOptions::new();
    o.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.share_mode(3).custom_flags(0x00200000);
    }
    let file = o.open(path)?;
    if file.metadata()?.len() != 0 {
        return Err(invalid());
    }
    file.try_lock().map_err(|_| invalid())?;
    Ok((dirs, file))
}
pub fn initialize(input: &Init) -> io::Result<Handle> {
    if !cfg!(windows) || !input.root.is_absolute() || input.root.exists() {
        return Err(invalid());
    }
    let c = &input.config;
    if c.executable.canonicalize()? != std::env::current_exe()?.canonicalize()? {
        return Err(invalid());
    }
    let mut index =
        Index::open(&c.package.index_root, &c.package.index_minimum).map_err(|_| invalid())?;
    if index.identity() != &c.package.index {
        return Err(invalid());
    }
    let mut nodes = MetadataStore::open(&c.package.metadata.root, &c.package.metadata.minimum)
        .map_err(|_| invalid())?;
    plan::validate(c, &mut index, &mut nodes)?;
    let index_minimum = index.checkpoint();
    let policy_minimum = tig_store::source_claims::policy_registry::Registry::open(
        &c.policy.root,
        &c.policy.minimum,
    )
    .map_err(|_| invalid())?
    .checkpoint();
    let enrolled = Enrolled {
        schema: SCHEMA.into(),
        config: c.clone(),
        owner: index.enrollment_profile().owner.clone(),
    };
    let empty = Tree::empty(&mut nodes)
        .map_err(|_| invalid())?
        .root()
        .clone();
    let configuration = put(&mut nodes, &enrolled)?;
    nodes.finish().map_err(|_| invalid())?;
    let mut journal = SourceJournal::create(&input.root, k("annex-merges")?, input.epoch.clone())
        .map_err(|_| invalid())?;
    fs::create_dir(input.root.join("io"))?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(input.root.join("controller.lock"))?
        .sync_all()?;
    save(
        &mut journal,
        &mut nodes,
        &k("configuration")?,
        &enrolled.owner,
        &enrolled,
        None,
        false,
    )?;
    save(
        &mut journal,
        &mut nodes,
        &k("head")?,
        &enrolled.owner,
        &Head {
            settled: false,
            index_minimum: index_minimum.clone(),
            policy_minimum: policy_minimum.clone(),
            cursor: c.initial_commit.clone(),
            deferred: empty,
            active: None,
        },
        None,
        false,
    )?;
    Ok(Handle {
        root: input.root.clone(),
        journal: journal.identity().clone(),
        minimum: journal.checkpoint(),
        metadata: retirement::MetadataBinding {
            root: c.package.metadata.root.clone(),
            minimum: nodes.checkpoint(),
        },
        configuration,
        index_minimum,
        policy_minimum,
    })
}
/// Materialize a bounded, repairable control copy from exact expected bytes.
/// These files are not authority: the journal-selected metadata objects are.
/// An exclusive writer and exact readback prevent a child consuming a partial
/// copy; a process crash can be repaired from the same immutable object.
#[cfg(windows)]
fn profile_file(root: &std::path::Path, bytes: &[u8]) -> io::Result<PathBuf> {
    use std::io::Write as _;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    if bytes.len() > 65536 {
        return Err(invalid());
    }
    let _dirs = crate::annex::legacy::hold_directories(root)?;
    let path = root.join(format!("{}.json", Digest::blob(bytes).as_str()));
    if path.try_exists()? {
        let file = locked_file(&path)?;
        if file.metadata()?.len() > 65536 {
            return Err(invalid());
        }
        if file.metadata()?.len() == bytes.len() as u64 && fs::read(&path)? == bytes {
            return Ok(path);
        }
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .custom_flags(0x00200000);
    let mut file = options.open(&path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 65536 || metadata.file_attributes() & 0x400 != 0 {
        return Err(invalid());
    }
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    let held = locked_file(&path)?;
    if held.metadata()?.len() != bytes.len() as u64 || fs::read(&path)? != bytes {
        return Err(invalid());
    }
    Ok(path)
}
#[cfg(not(windows))]
fn profile_file(_: &std::path::Path, _: &[u8]) -> io::Result<PathBuf> {
    Err(invalid())
}
fn child<T: serde::de::DeserializeOwned>(
    s: &Session,
    command: ControlCommand,
    input: &impl Serialize,
) -> io::Result<T> {
    let b = encode(input)?;
    if b.len() > 65536 {
        return Err(invalid());
    }
    let root = s.handle.root.join("io");
    let _dirs = crate::annex::legacy::hold_directories(&root)?;
    let path = profile_file(&root, &b)?;
    let file = locked_file(&path)?;
    if file.metadata()?.len() != b.len() as u64 || fs::read(&path)? != b {
        return Err(invalid());
    }
    let c = &s.enrolled.config;
    let out = Runner::new(c.caller.clone())?.control_command(
        &c.executable,
        &root,
        command,
        &path,
        c.limits.child_millis,
    )?;
    if !out.success {
        return Err(invalid());
    }
    let value = tig_core::identity::parse_json(&out.stdout).map_err(|_| invalid())?;
    serde_json::from_value(value).map_err(|_| invalid())
}
fn report(
    s: &Session,
    r: &Request,
    status: Status,
    outcomes: Vec<retirement::Report>,
    deferred: &Object,
) -> io::Result<Report> {
    // Point-free bounded metadata traversal only, not payload/cache discovery.
    let mut nodes = MetadataStore::open(&s.handle.metadata.root, &s.nodes.checkpoint())
        .map_err(|_| invalid())?;
    Ok(Report {
        work: None,
        blocked_by: None,
        status,
        operation: r.operation.clone(),
        commit: r.commit.clone(),
        handle: s.handle(),
        outcomes,
        deferred_objects: entries(&mut nodes, deferred)?.len(),
        reclaimed_bytes: None,
    })
}
fn finish(s: &mut Session, r: &Request, saved: &mut Saved, done: Finished) -> io::Result<Report> {
    if s.journal
        .source(&run_key(&r.operation)?)
        .map_err(|_| invalid())?
        .is_none()
    {
        s.save(&r.operation, saved)?;
    }
    saved.terminal = Some(done.clone());
    s.save(&r.operation, saved)?;
    finalize(s, r, saved, false)
}
fn finalize(s: &mut Session, r: &Request, saved: &Saved, historical: bool) -> io::Result<Report> {
    let done = saved.terminal.as_ref().ok_or_else(invalid)?;
    if s.head
        .active
        .as_ref()
        .is_some_and(|a| a.operation == r.operation)
    {
        if !s.head.settled {
            s.head.settled = true;
            s.head.index_minimum = done.index_minimum.clone();
            s.head.policy_minimum = done.policy_minimum.clone();
            s.head(Some(&saved.head_budget))?;
        }
        s.head = Head {
            settled: false,
            index_minimum: done.index_minimum.clone(),
            policy_minimum: done.policy_minimum.clone(),
            cursor: done.cursor.clone(),
            deferred: done.deferred.clone(),
            active: None,
        };
        s.head(Some(&saved.head_budget))?;
    }
    progress_budget::release(&mut s.journal, &saved.run_budget).map_err(|_| invalid())?;
    progress_budget::release(&mut s.journal, &saved.head_budget).map_err(|_| invalid())?;
    report(
        s,
        r,
        if historical {
            Status::Historical
        } else {
            done.status.clone()
        },
        read(&s.nodes, &done.outcomes)?,
        &done.deferred,
    )
}
fn settle_effects(
    s: &mut Session,
    profiles: &[retirement::Profile],
    reconcile_only: bool,
    cancel: bool,
) -> io::Result<Vec<retirement::Report>> {
    let mut result = Vec::new();
    for p in profiles {
        let action = if cancel {
            ControlCommand::Cancel
        } else if reconcile_only {
            ControlCommand::Reconcile
        } else {
            ControlCommand::Retire
        };
        let mut response: retirement::Report = match child(s, action, p) {
            Ok(v) => v,
            Err(_) => child(s, ControlCommand::Reconcile, p)?,
        };
        if matches!(response.status, retirement::Status::Uncertain) {
            response = child(s, ControlCommand::Reconcile, p)?;
        }
        if response.object != p.request.object
            || response.store != p.request.store
            || response.reclaimed_bytes.is_some()
        {
            return Err(invalid());
        }
        let pending = matches!(response.status, retirement::Status::Uncertain);
        result.push(response);
        if pending {
            break;
        }
    }
    Ok(result)
}
pub fn execute(request: &Request, mode: Mode) -> io::Result<Report> {
    catchup::execute(request, mode)
}
fn drive(
    s: &mut Session,
    request: &Request,
    mode: Mode,
    controller: &Controller,
) -> io::Result<Report> {
    if let Some(saved) = s.saved(&request.operation)? {
        let recipe: Recipe = read(&s.nodes, &saved.recipe)?;
        if recipe.after != request.commit {
            return Err(invalid());
        }
        if saved.terminal.is_some() {
            return finalize(s, request, &saved, true);
        }
    }
    if matches!(mode, Mode::Inspect) {
        staging::advance(s, request)?;
        return report(s, request, Status::Pending, vec![], &s.head.deferred);
    }
    if s.head.active.is_none() {
        let head = s
            .journal
            .source(&k("head")?)
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        let recipe = plan::build(s, request)?;
        if head.predecessors.len() + 3 > 64 {
            maintenance::trim(&mut s.journal, &mut s.nodes, &s.enrolled.owner)?;
        }
        let recipe = put(&mut s.nodes, &recipe)?;
        s.nodes.finish().map_err(|_| invalid())?;
        s.head.active = Some(Active {
            operation: request.operation.clone(),
            commit: request.commit.clone(),
            recipe,
        });
        s.head(None)?;
    }
    let active = s.head.active.clone().ok_or_else(invalid)?;
    if active.commit != request.commit {
        return Err(invalid());
    }
    let recipe: Recipe = read(&s.nodes, &active.recipe)?;
    let mut saved = match s.saved(&request.operation)? {
        Some(v) => v,
        None => Saved {
            index_minimum: s.index()?.checkpoint(),
            policy_minimum: stronger(&s.head.policy_minimum, &s.handle.policy_minimum)?,
            recipe: active.recipe.clone(),
            head_budget: reserve(
                &mut s.journal,
                &s.enrolled.owner,
                &k("head")?,
                &request.operation,
                "head-budget",
            )?,
            run_budget: reserve(
                &mut s.journal,
                &s.enrolled.owner,
                &run_key(&request.operation)?,
                &request.operation,
                "run-budget",
            )?,
            profiles: None,
            terminal: None,
        },
    };
    s.handle.index_minimum = stronger(&s.handle.index_minimum, &saved.index_minimum)?;
    s.handle.policy_minimum = stronger(&s.handle.policy_minimum, &saved.policy_minimum)?;
    staging::advance(s, request)?;
    saved.index_minimum = stronger(&saved.index_minimum, &s.handle.index_minimum)?;
    saved.policy_minimum = stronger(&saved.policy_minimum, &s.handle.policy_minimum)?;
    let stale = plan::current(&s.enrolled.config, &request.commit).is_err();
    if matches!(mode, Mode::Cancel) || stale || staging::cancelled(s, request)? {
        let profiles = match &saved.profiles {
            Some(root) => read(&s.nodes, root)?,
            None => staging::cancel(s, request)?,
        };
        let outcomes = settle_effects(s, &profiles, true, true)?;
        if outcomes
            .iter()
            .any(|r| matches!(r.status, retirement::Status::Uncertain))
        {
            return report(s, request, Status::Pending, outcomes, &recipe.cleanup);
        }
        let mut index = s.index()?;
        let selected = index
            .actor(&mut s.nodes, &s.enrolled.config.current)
            .map_err(|_| invalid())?
            .selection;
        let cursor = if selected == recipe.current_after {
            recipe.after.clone()
        } else if selected == recipe.current_before {
            recipe.before.clone()
        } else {
            return Err(invalid());
        };
        let index_minimum = index.checkpoint();
        let policy_minimum = saved.policy_minimum.clone();
        let outcomes = put(&mut s.nodes, &outcomes)?;
        return finish(
            s,
            request,
            &mut saved,
            Finished {
                index_minimum,
                policy_minimum,
                status: Status::Superseded,
                cursor,
                deferred: recipe.cleanup.clone(),
                outcomes,
            },
        );
    }
    if saved.profiles.is_none() {
        let has_stage = staging::exists(s, request)?;
        let c = s.config()?;
        let inventory = if !has_stage {
            let tasks: Vec<CopyTask> = read(&s.nodes, &recipe.copies)?;
            let caller = Runner::new(c.caller.clone())?;
            for task in tasks {
                let d = c
                    .destinations
                    .iter()
                    .find(|d| {
                        d.enrollment
                            .control
                            .as_ref()
                            .is_some_and(|b| b.store == task.store)
                    })
                    .ok_or_else(invalid)?;
                if super::copy(
                    &caller,
                    &Runner::new(d.enrollment.clone())?,
                    &AnnexKey::from_object(&task.object),
                    None,
                    &Remote::parse(&d.remote)?,
                    0,
                    c.limits.verification_bytes,
                )? != super::TransferOutcome::VerifiedPresent
                {
                    return report(s, request, Status::Pending, vec![], &recipe.cleanup);
                }
            }
            Some(plan::inventory(&c, &mut s.nodes, &recipe)?)
        } else {
            None
        };
        let lease = hold_ref(&c, &request.commit)?;
        let result = (|| {
            if !has_stage && let Some(root) = &recipe.projection {
                let batch: Batch = read(&s.nodes, root)?;
                s.index()?
                    .update_batch(&batch, &mut s.nodes)
                    .map_err(|_| invalid())?;
            }
            let profiles = staging::prepare(
                s,
                request,
                &recipe,
                inventory,
                !matches!(mode, Mode::Project),
            )?;
            if let Some(profiles) = profiles {
                saved.index_minimum = s.index()?.checkpoint();
                for p in &profiles {
                    if let Some(policy) = &p.policy {
                        saved.policy_minimum =
                            stronger(&saved.policy_minimum, &policy.authority.minimum)?;
                    }
                }
                saved.profiles = Some(put(&mut s.nodes, &profiles)?);
                s.nodes.finish().map_err(|_| invalid())?;
                s.save(&request.operation, &saved)?;
                s.handle.index_minimum = stronger(&s.handle.index_minimum, &saved.index_minimum)?;
                s.handle.policy_minimum =
                    stronger(&s.handle.policy_minimum, &saved.policy_minimum)?;
            }
            Ok::<_, io::Error>(())
        })();
        if controller.children_stopped(5000).is_err() {
            std::mem::forget(lease);
            return report(s, request, Status::Pending, vec![], &recipe.cleanup);
        }
        drop(lease);
        result?;
    }
    if matches!(mode, Mode::Project) {
        staging::advance(s, request)?;
        let mut value = report(s, request, Status::Projected, vec![], &recipe.cleanup)?;
        value.work = staging::input_root(s, request)?;
        return Ok(value);
    }
    if matches!(mode, Mode::Prepare) {
        let mut value = report(s, request, Status::Prepared, vec![], &recipe.cleanup)?;
        value.work = saved.profiles.clone();
        return Ok(value);
    }
    let lease = hold_ref(&s.enrolled.config, &request.commit)?;
    let profiles: Vec<retirement::Profile> =
        read(&s.nodes, saved.profiles.as_ref().ok_or_else(invalid)?)?;
    let result = settle_effects(s, &profiles, false, false);
    if controller.children_stopped(5000).is_err() {
        std::mem::forget(lease);
        return report(s, request, Status::Pending, vec![], &recipe.cleanup);
    }
    drop(lease);
    let outcomes = result?;
    if outcomes.len() != profiles.len()
        || outcomes
            .iter()
            .any(|r| matches!(r.status, retirement::Status::Uncertain))
    {
        return report(s, request, Status::Pending, outcomes, &recipe.cleanup);
    }
    let mut remaining = entries(&mut s.nodes, &recipe.cleanup)?;
    for object in remaining.values().cloned().collect::<Vec<_>>() {
        let matching: Vec<_> = outcomes.iter().filter(|r| r.object == object).collect();
        if !matching.is_empty()
            && matching.iter().all(|r| {
                matches!(
                    r.status,
                    retirement::Status::Removed | retirement::Status::HistoricalRemoved
                )
            })
        {
            remaining.remove(&retired(&object)?);
        }
    }
    let deferred = Tree::build(&remaining, &mut s.nodes, tree_limits())
        .map_err(|_| invalid())?
        .0
        .root()
        .clone();
    let index_minimum = s.index()?.checkpoint();
    let policy_minimum = saved.policy_minimum.clone();
    let outcomes = put(&mut s.nodes, &outcomes)?;
    finish(
        s,
        request,
        &mut saved,
        Finished {
            index_minimum,
            policy_minimum,
            status: if remaining.is_empty() {
                Status::Complete
            } else {
                Status::Deferred
            },
            cursor: recipe.after,
            deferred,
            outcomes,
        },
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn partial_control_copy_is_reconstructed_from_exact_expected_bytes() {
        let root = tempfile::tempdir().unwrap();
        let bytes = br#"{"fixture":"complete-control-profile"}"#;
        let path = root
            .path()
            .join(format!("{}.json", Digest::blob(bytes).as_str()));
        fs::write(&path, &bytes[..7]).unwrap();
        assert_eq!(profile_file(root.path(), bytes).unwrap(), path);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::write(&path, b"corrupted control copy").unwrap();
        assert_eq!(profile_file(root.path(), bytes).unwrap(), path);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::write(&path, vec![0; 65537]).unwrap();
        assert!(profile_file(root.path(), bytes).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), 65537);
    }
}
