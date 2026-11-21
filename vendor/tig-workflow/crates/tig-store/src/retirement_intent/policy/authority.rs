use super::*;
use crate::source_claims::{
    Checkpoint,
    coordinated::accepted_head::{HeldHead, hold_current},
    policy_registry::{HeldPolicy, Registry},
    protection::Index,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// Native trusted configuration only, saved in private controller recipes or
/// supplied by an admitted runtime. No root/credential is read from policy data.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityBindings {
    pub registry_root: PathBuf,
    pub registry: Digest,
    pub registry_minimum: Checkpoint,
    pub slot: Key,
    pub index_root: PathBuf,
    pub index: Digest,
    pub index_minimum: Checkpoint,
    pub source_root: PathBuf,
    pub source: Digest,
    pub source_minimum: Checkpoint,
}
struct State {
    registry: Registry,
    index: Index,
    source: SourceJournal,
}
pub struct NativeAuthority {
    bindings: AuthorityBindings,
    required_registry: Option<Checkpoint>,
    state: Mutex<Option<State>>,
}
impl std::fmt::Debug for NativeAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeAuthority")
            .field("registry", &self.bindings.registry)
            .finish_non_exhaustive()
    }
}
impl NativeAuthority {
    /// Lazy: completed/historical work need not reopen an offline authority.
    pub fn new(bindings: AuthorityBindings) -> Result<Self> {
        if !bindings.registry_root.is_absolute()
            || !bindings.index_root.is_absolute()
            || !bindings.source_root.is_absolute()
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            bindings,
            required_registry: None,
            state: Mutex::new(None),
        })
    }
    pub fn require_registry_minimum(mut self, minimum: Checkpoint) -> Self {
        self.required_registry = Some(minimum);
        self
    }
    /// Last checked native point, without reopening an offline authority.
    pub fn registry_checkpoint(&self) -> Option<Checkpoint> {
        self.state
            .lock()
            .ok()?
            .as_ref()
            .map(|s| s.registry.checkpoint())
    }
    pub fn identity(&self) -> &Digest {
        &self.bindings.registry
    }
    pub(super) fn acquire(
        &self,
        ctx: &ObservedIntent,
        nodes: &impl NodeStore,
    ) -> Result<EffectHold> {
        let context = ctx.execution_profile().ok_or(Error::Missing)?;
        let proof = Proof::load(nodes, context, ctx.review(), ctx.decision())?;
        if proof.grant().registry != self.bindings.registry
            || proof.grant().slot != self.bindings.slot
            || proof.owner() != ctx.owner()
        {
            return Err(Error::Owner);
        }
        let mut state = self.state.lock().map_err(|_| Error::Invalid)?;
        if state.is_none() {
            let registry = Registry::open(
                &self.bindings.registry_root,
                &self.bindings.registry_minimum,
            )
            .map_err(|_| Error::Stale)?;
            if let Some(minimum) = &self.required_registry {
                drop(
                    Registry::open(&self.bindings.registry_root, minimum)
                        .map_err(|_| Error::Stale)?,
                );
            }
            let index = Index::open(&self.bindings.index_root, &self.bindings.index_minimum)
                .map_err(|_| Error::Stale)?;
            let source =
                SourceJournal::open(&self.bindings.source_root, &self.bindings.source_minimum)
                    .map_err(Error::Journal)?;
            if registry.identity() != &self.bindings.registry
                || index.identity() != &self.bindings.index
                || source.identity() != &self.bindings.source
            {
                return Err(Error::Invalid);
            }
            *state = Some(State {
                registry,
                index,
                source,
            });
        }
        let State {
            registry,
            index,
            source,
        } = state.as_mut().ok_or(Error::Missing)?;
        if registry.binding().owner() != ctx.owner()
            || index.enrollment_profile().owner != *ctx.owner()
            || index.enrollment_profile().subject != ctx.decision().approval().owner
            || source.identity() != &ctx.journal
        {
            return Err(Error::Owner);
        }
        let registration = registry
            .hold(&self.bindings.slot, Some(&proof.grant().version), nodes)
            .map_err(|_| Error::Stale)?;
        if registration.owner() != ctx.owner()
            || registration.policy() != proof.policy()
            || registration.catalogue() != proof.providers()
        {
            return Err(Error::Invalid);
        }
        let head = hold_current(index, source).map_err(Error::Journal)?;
        head.require_run(proof.run()).map_err(Error::Journal)?;
        registry
            .check_run(
                &registration,
                &proof,
                context,
                ctx.review(),
                ctx.decision(),
                nodes,
            )
            .map_err(|_| Error::Budget)?;
        Ok(EffectHold {
            _registration: registration,
            _head: head,
        })
    }
}
/// Scoped current registration and source exclusion. Hold only through the new
/// physical effect boundary, after payload verification; no serialization/clone.
pub struct EffectHold {
    _registration: HeldPolicy,
    _head: HeldHead,
}

impl ObservedIntent {
    pub fn attach_policy_authority(
        mut self,
        nodes: &impl NodeStore,
        authority: Arc<NativeAuthority>,
    ) -> Result<Self> {
        let context = self.execution_profile().ok_or(Error::Missing)?;
        let proof = Proof::load(nodes, context, self.review(), self.decision())?;
        if authority.identity() != &proof.grant().registry {
            return Err(Error::Invalid);
        }
        self.policy_authority = Some(authority);
        Ok(self)
    }
}
