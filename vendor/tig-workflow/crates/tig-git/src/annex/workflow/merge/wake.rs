//! Normalized event hints request work; they never select or authorize effects.
use super::*;
use crate::source::protection_work;
use tig_store::{metadata::MetadataReader, source_claims::policy_registry::Registry};

pub const PROFILE: &str = "tig.annex-merge-wake.experimental-1";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Push,
    CatchUp,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    pub kind: Kind,
    pub repository: u64,
    pub reference: Option<String>,
    pub delivery: Option<String>,
    pub commit_hint: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: String,
    /// Trusted runner enrollment, not authentication of a hosted repository.
    pub repository: u64,
    pub reference: String,
    pub handle: Handle,
    pub trigger: Trigger,
}
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Proposed,
    Resume,
    Settled,
    Deferred,
}
#[derive(Serialize)]
pub struct Observation {
    pub schema: &'static str,
    pub repository: u64,
    pub reference: String,
    pub current_commit: String,
    pub settled_commit: String,
    pub status: Status,
    pub deferred_objects: usize,
    pub handle: Handle,
    pub request: Option<Request>,
    pub policy_version: source_claims::Version,
    pub execution_permission: &'static str,
    pub hosted_current_authority: &'static str,
}
fn bounded(value: &Option<String>, limit: usize) -> bool {
    value
        .as_ref()
        .is_none_or(|v| !v.is_empty() && v.len() <= limit && !v.chars().any(char::is_control))
}
fn validate(p: &Input) -> io::Result<()> {
    if p.schema != PROFILE
        || p.repository == 0
        || p.repository != p.trigger.repository
        || !p.reference.starts_with("refs/heads/")
        || p.reference.len() > 256
        || p.reference.chars().any(char::is_control)
        || !bounded(&p.trigger.delivery, 256)
        || !bounded(&p.trigger.commit_hint, 128)
        || match p.trigger.kind {
            Kind::Push => p.trigger.reference.as_ref() != Some(&p.reference),
            Kind::CatchUp => p.trigger.reference.is_some() || p.trigger.commit_hint.is_some(),
        }
        || encode(p)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "wake encoding refused"))?
            .len()
            > 65536
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "wake scope refused",
        ));
    }
    Ok(())
}
pub fn observe(p: &Input) -> io::Result<Observation> {
    validate(p)?; // Before any authority, Git or provider access.
    let mut journal =
        SourceJournal::open(&p.handle.root, &p.handle.minimum).map_err(|_| invalid())?;
    let mut nodes = MetadataReader::open(&p.handle.metadata.root, &p.handle.metadata.minimum)
        .map_err(|_| invalid())?;
    let enrolled = store::enrollment(&p.handle, &mut journal, &nodes)?;
    let c = &enrolled.config;
    if c.reference != p.reference {
        return Err(invalid());
    }
    let head_source = journal
        .source(&k("head")?)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    let head: Head =
        store::selected(&mut journal, &nodes, &k("head")?, &enrolled.owner)?.ok_or_else(invalid)?;
    let mut handle = p.handle.clone();
    handle.index_minimum = stronger(&handle.index_minimum, &head.index_minimum)?;
    handle.policy_minimum = stronger(&handle.policy_minimum, &head.policy_minimum)?;
    // Check every independently retained witness, without issuing a command.
    for min in [
        &c.package.index_minimum,
        &head.index_minimum,
        &p.handle.index_minimum,
    ] {
        let current = Index::open(&c.package.index_root, min).map_err(|_| invalid())?;
        if current.identity() != &c.package.index || !current.supports_effect_fences() {
            return Err(invalid());
        }
        handle.index_minimum = stronger(&handle.index_minimum, &current.checkpoint())?;
    }
    let mut index =
        Index::open(&c.package.index_root, &handle.index_minimum).map_err(|_| invalid())?;
    for min in [
        &c.policy.minimum,
        &head.policy_minimum,
        &p.handle.policy_minimum,
    ] {
        let r = Registry::open(&c.policy.root, min).map_err(|_| invalid())?;
        if r.identity() != &c.policy.registry {
            return Err(invalid());
        }
        handle.policy_minimum = stronger(&handle.policy_minimum, &r.checkpoint())?;
    }
    let mut registry =
        Registry::open(&c.policy.root, &handle.policy_minimum).map_err(|_| invalid())?;
    registry
        .require_effect_index(&index)
        .map_err(|_| invalid())?;
    let held = registry
        .hold(&c.policy.slot, Some(&c.policy.version), &nodes)
        .map_err(|_| invalid())?;
    let frame = registry
        .effect_frame(&mut index, &nodes)
        .map_err(|_| invalid())?;
    let policy = held.policy().config();
    if frame.source != index.policy_source(&c.current).map_err(|_| invalid())?
        || held.owner() != &enrolled.owner
        || policy.owner != index.enrollment_profile().subject
        || policy.namespace != *index.identity()
        || policy.source != frame.source
        || policy.authority != *index.identity()
        || policy.epoch != *index.identity()
    {
        return Err(invalid());
    }
    let git = plan::reader(c)?;
    let commit =
        protection_work::observe_current(&git, &plan::binding(c)).map_err(|_| invalid())?;
    let commit = commit.as_str().to_owned();
    let _reference = hold_ref(c, &commit)?;
    if head.active.is_none() && head.cursor == commit {
        let snapshot = git
            .commit(crate::source::GitId::parse(&commit).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
        let selected = git
            .extract_annex(
                &snapshot,
                crate::source::Scope::new(c.scope.clone()).map_err(|_| invalid())?,
            )
            .map_err(|_| invalid())?;
        if !selected.complete()
            || index
                .actor(&mut nodes, &c.current)
                .map_err(|_| invalid())?
                .selection
                .context
                != selected.selection_digest().map_err(|_| invalid())?
        {
            return Err(invalid());
        }
    }

    let deferred_objects = Tree::from_root(head.deferred.clone())
        .entries(&mut nodes, tree_limits())
        .map_err(|_| invalid())?
        .0
        .len();
    let (status, operation) = if let Some(active) = &head.active {
        if active.commit == commit {
            (Status::Resume, Some(active.operation.clone()))
        } else {
            (
                Status::Proposed,
                Some(operation(&handle, head_source.revision, &commit)?),
            )
        }
    } else if head.cursor == commit {
        (
            if deferred_objects == 0 {
                Status::Settled
            } else {
                Status::Deferred
            },
            None,
        )
    } else {
        (
            Status::Proposed,
            Some(operation(&handle, head_source.revision, &commit)?),
        )
    };
    // A moving control head is not an observation to act on. Git ref is held;
    // execution still reacquires all original guards after this lease ends.
    if journal.source(&k("head")?).map_err(|_| invalid())?.as_ref() != Some(&head_source) {
        return Err(invalid());
    }
    handle.minimum = journal.checkpoint();
    handle.metadata.minimum = nodes.checkpoint().clone();
    handle.index_minimum = stronger(&handle.index_minimum, &index.checkpoint())?;
    handle.policy_minimum = stronger(&handle.policy_minimum, &registry.checkpoint())?;
    let request = operation.map(|operation| Request {
        handle: handle.clone(),
        operation,
        commit: commit.clone(),
    });
    let result = Observation {
        schema: "tig.annex-merge-observation.experimental-1",
        repository: p.repository,
        reference: p.reference.clone(),
        current_commit: commit,
        settled_commit: head.cursor,
        status,
        deferred_objects,
        handle,
        request,
        policy_version: held.version().clone(),
        execution_permission: "none",
        hosted_current_authority: "unverified",
    };
    if encode(&result)?.len() > 65536 {
        return Err(invalid());
    }
    Ok(result)
}
fn operation(handle: &Handle, head_revision: u64, commit: &str) -> io::Result<Digest> {
    Ok(Digest::blob(&encode(&(
        PROFILE,
        "request",
        &handle.journal,
        &handle.configuration,
        head_revision,
        commit,
    ))?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> serde_json::Value {
        let d = Digest::blob(b"synthetic-wake-scope");
        let checkpoint = serde_json::json!({"genesis":d,"hash":d,"sequence":0});
        let root = std::env::temp_dir().join(format!("unopened-wake-{}", std::process::id()));
        serde_json::json!({"schema":PROFILE,"repository":7,"reference":"refs/heads/main",
            "handle":{"root":root,"journal":d,"minimum":checkpoint,"configuration":{"bytes":1,"sha256":d},
                "metadata":{"root":root.join("metadata"),"minimum":{"identity":d,"generation":0,"nodes":0,"bytes":0}},
                "index_minimum":checkpoint,"policy_minimum":checkpoint},
            "trigger":{"kind":"push","repository":7,"reference":"refs/heads/main","delivery":"delivery-1","commit_hint":"old-hint"}})
    }
    #[test]
    fn invalid_wake_scope_refuses_before_unavailable_authority_io() {
        let base = profile();
        let valid: Input = serde_json::from_value(base.clone()).unwrap();
        assert!(validate(&valid).is_ok());
        for (field, value) in [
            ("repository", serde_json::json!(8)),
            ("reference", serde_json::json!("refs/heads/foreign")),
            ("commit_hint", serde_json::json!("x".repeat(129))),
            ("delivery", serde_json::json!("bad\ndelivery")),
        ] {
            let mut bad = base.clone();
            bad["trigger"][field] = value;
            let p: Input = serde_json::from_value(bad).unwrap();
            assert_eq!(
                observe(&p).err().unwrap().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        let mut bad = base.clone();
        bad["trigger"]["kind"] = serde_json::json!("pull-request");
        assert!(serde_json::from_value::<Input>(bad).is_err());
        let mut bad = base.clone();
        bad["trigger"]["root"] = serde_json::json!("event-root");
        assert!(serde_json::from_value::<Input>(bad).is_err());
        let mut bad = base.clone();
        bad["handle"]["root"] = serde_json::json!("x".repeat(65536));
        let p: Input = serde_json::from_value(bad).unwrap();
        assert_eq!(
            observe(&p).err().unwrap().kind(),
            io::ErrorKind::InvalidInput
        );
        let mut bad = base;
        bad["trigger"]["kind"] = serde_json::json!("catch-up");
        let p: Input = serde_json::from_value(bad).unwrap();
        assert_eq!(
            observe(&p).err().unwrap().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
