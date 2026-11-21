//! Shared private enrollment and existing placement ports for local controllers.
use super::{absolute, placement_cli, profiles::Storage};
use git_asset_core::{Digest, identity, tree::NodeStore};
use git_asset_store::{
    Key,
    existing::{Profile as SourceProfile, ReadCache},
    ledger::CapacityLedger,
    local::LocalStore,
    placement::{self, Cursor, Local},
    source_claims::admission::Namespace,
};
use std::{fs, path::Path};
pub(super) struct Binding<'a> {
    pub root: &'a Path,
    pub owner: &'a Key,
    pub authority: &'a Digest,
    pub namespace: &'a Digest,
    pub policy: &'a Digest,
}
pub(super) struct Open {
    profile: Storage,
    namespaces: Vec<Namespace>,
    source_index: usize,
    stores: Vec<LocalStore>,
    ledgers: Vec<CapacityLedger>,
    seed: ReadCache,
    seed_identity: Digest,
}
impl Open {
    pub fn new(
        profile: Storage,
        namespace: Namespace,
        binding: Binding<'_>,
    ) -> Result<Self, &'static str> {
        if profile.schema != "tig.placement-profile.design-1"
            || profile.providers.len() != profile.config.providers.len()
            || profile.namespaces.len() != profile.config.namespaces.len()
            || &profile.config.owner != binding.owner
            || &profile.config.authority != binding.authority
            || namespace.identity() != binding.namespace
            || !profile
                .config
                .namespaces
                .iter()
                .any(|n| &n.namespace == binding.namespace && &n.policy == binding.policy)
        {
            return Err("authority");
        }
        let seed_identity = Digest::blob(
            &identity::canonical_json(&profile.byte_source).map_err(|_| "invalid_document")?,
        );
        if seed_identity != profile.config.seed {
            return Err("authority");
        }
        let seed_profile: SourceProfile =
            serde_json::from_value(profile.byte_source.clone()).map_err(|_| "invalid_document")?;
        let seed = ReadCache::new(seed_profile).map_err(|_| "source_unavailable")?;
        let mut namespaces = profile
            .namespaces
            .iter()
            .map(|n| {
                absolute(&n.root)?;
                Namespace::open(&n.root, &n.minimum).map_err(|_| "journal_unavailable")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let source_index = namespaces
            .iter()
            .position(|n| n.identity() == binding.namespace)
            .ok_or("authority")?;
        if fs::canonicalize(binding.root).map_err(|_| "journal_unavailable")?
            != fs::canonicalize(&profile.namespaces[source_index].root)
                .map_err(|_| "journal_unavailable")?
        {
            return Err("authority");
        }
        namespaces[source_index] = namespace;
        let stores = profile
            .providers
            .iter()
            .map(|p| {
                absolute(&p.root)?;
                LocalStore::open(&p.root).map_err(|_| "provider_unavailable")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ledgers = profile
            .providers
            .iter()
            .map(|p| {
                absolute(&p.ledger_root)?;
                CapacityLedger::open(&p.ledger_root, &p.minimum).map_err(|_| "capacity_unavailable")
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            profile,
            namespaces,
            source_index,
            stores,
            ledgers,
            seed,
            seed_identity,
        })
    }
    pub fn namespace(&mut self) -> &mut Namespace {
        &mut self.namespaces[self.source_index]
    }
    pub fn preflight(&mut self, nodes: &impl NodeStore) -> Result<(), &'static str> {
        self.run(nodes, None, false).map(|_| ())
    }
    pub fn apply(
        &mut self,
        nodes: &impl NodeStore,
        cursor: Option<&Cursor>,
    ) -> Result<(placement::Plan, placement::Report), &'static str> {
        let (plan, report) = self.run(nodes, cursor, true)?;
        Ok((plan, report.ok_or("invalid_document")?))
    }
    fn run(
        &mut self,
        nodes: &impl NodeStore,
        cursor: Option<&Cursor>,
        apply: bool,
    ) -> Result<(placement::Plan, Option<placement::Report>), &'static str> {
        let mut places = self
            .stores
            .iter()
            .zip(self.ledgers.iter_mut())
            .zip(self.profile.config.providers.iter())
            .map(|((store, ledger), p)| Local {
                store,
                ledger,
                job: &p.job,
            })
            .collect::<Vec<_>>();
        let mut execution = placement::Execution {
            config: &self.profile.config,
            namespaces: &mut self.namespaces,
            nodes,
            places: &mut places,
            seed: &mut self.seed,
            seed_identity: &self.seed_identity,
            allow_copy: apply && self.profile.allow_copy,
            allow_eviction: apply && self.profile.allow_eviction,
            allow_reacquisition: apply && self.profile.allow_reacquisition,
            allow_cancellation: apply && self.profile.allow_cancellation,
        };
        let plan = execution
            .plan(cursor)
            .map_err(placement_cli::placement_code)?;
        let report = if apply {
            Some(
                execution
                    .apply(&plan)
                    .map_err(placement_cli::placement_code)?,
            )
        } else {
            None
        };
        Ok((plan, report))
    }
}
