//! Native public SDK lifecycle. REEL owns task decisions, providers and cleanup.
use crate::{
    GuardedInputs, Preview, ProtectedInputs, Snapshot,
    binding::{Binding, object},
    candidates::{Candidates, Prepared, RETURN_PACKET, RetainedBatch},
    claims::admission_error,
    error, native_error,
    retention::{Budget, Held, ProtectedSet, ProtectionBudget},
};
use git_asset::annex::{
    managed,
    publication::{Publisher, Resources},
};
use git_asset_core::{
    LogicalPath, Object,
    tree::{MemoryNodes, NodeStore},
};
use git_asset_store::existing::ByteSource;
use reel_workflow::{
    Artifact, ErrorKind, Result, ReturnedResult,
    handoff::{Lifecycle as LifecycleTrait, ProtectedJob, Ticket},
};
use std::{collections::BTreeMap, io::Read};
pub enum Access {
    Active {
        snapshot: Snapshot,
        read_budget: u64,
    },
    RetainedOnly,
}
pub struct Configuration {
    pub expected: Ticket,
    pub tool: Artifact,
    pub profile: managed::Profile,
    pub controls: Budget,
    pub retained: ProtectionBudget,
    pub access: Access,
}
pub struct Lifecycle<'r, S: ByteSource> {
    configuration: Configuration,
    publisher: Publisher,
    resources: Resources<'r>,
    source: S,
}
pub struct Guard<'a, 'r, S: ByteSource> {
    runtime: &'a mut Lifecycle<'r, S>,
    binding: Binding,
    _controls: Held,
    _inputs: Option<ProtectedInputs>,
    return_controls: Option<Held>,
    prepared: Option<Prepared>,
    retained: Option<RetainedBatch>,
    final_controls: Option<ProtectedSet>,
}
fn path(s: &str) -> Result<LogicalPath> {
    LogicalPath::parse(s).map_err(|_| error(ErrorKind::Invalid, "public-lifecycle-path"))
}
fn read_binding(profile: &managed::Profile, a: &Artifact) -> Result<Vec<u8>> {
    let object = object(a)?;
    if object.bytes > 128 * 1024 {
        return Err(error(ErrorKind::Budget, "public-lifecycle-binding"));
    }
    let mut reader = managed::Reader::new(profile.clone()).map_err(native_error)?;
    let mut held = reader.open(&object).map_err(native_error)?;
    let mut bytes = Vec::with_capacity(object.bytes as usize);
    held.by_ref()
        .take(object.bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error(ErrorKind::NeedsRecovery, "public-lifecycle-binding-read"))?;
    if Object::from_bytes(&bytes) != object {
        return Err(error(ErrorKind::Integrity, "public-lifecycle-binding"));
    }
    Ok(bytes)
}
impl<'r, S: ByteSource> Lifecycle<'r, S> {
    pub fn into_configuration(self) -> Configuration {
        self.configuration
    }
    /// Construction is pure; expected Ticket/tool come from trusted original control,
    /// not hashes declared by a returned payload. No routes or jobs are replaced.
    pub fn new(
        configuration: Configuration,
        publisher: Publisher,
        resources: Resources<'r>,
        source: S,
    ) -> Result<Self> {
        reel_workflow::check_packet(&configuration.expected.packet)?;
        object(&configuration.expected.native_binding)?;
        object(&configuration.tool)?;
        if configuration.expected.lifecycle_tool != configuration.tool {
            return Err(error(ErrorKind::Denied, "public-lifecycle-tool"));
        }
        Ok(Self {
            configuration,
            publisher,
            resources,
            source,
        })
    }
}
impl<'r, S: ByteSource> LifecycleTrait for Lifecycle<'r, S> {
    type Guard<'a>
        = Guard<'a, 'r, S>
    where
        Self: 'a;
    fn reopen<'a>(&'a mut self, ticket: &Ticket) -> Result<Self::Guard<'a>> {
        if ticket != &self.configuration.expected {
            return Err(error(ErrorKind::Conflict, "public-lifecycle-ticket"));
        }
        let raw = read_binding(&self.configuration.profile, &ticket.native_binding)?;
        let binding = Binding::decode(&raw, &object(&ticket.native_binding)?)?;
        let request = binding.control_selection(&mut MemoryNodes::default())?;
        let controls = binding.control_seed().hold(
            &self.configuration.profile,
            &request,
            self.resources.metadata,
            &self.resources.limits,
            self.configuration.controls,
        )?;
        let preview = Preview::decode(controls.bytes(&path("control/preview")?)?)?;
        binding.verify(ticket, &preview, controls.bytes(&path("control/packet")?)?)?;
        // Retained-only recovery still proves that this ORIGINAL input job
        // was admitted; a completed output is not a substitute for that proof.
        let claim = binding.claim();
        let member = self
            .configuration
            .profile
            .binding
            .members()
            .iter()
            .find(|m| &m.namespace == claim.minimum().identity())
            .ok_or_else(|| error(ErrorKind::Denied, "public-lifecycle-job-roster"))?;
        let location = self
            .configuration
            .profile
            .members
            .iter()
            .find(|m| m.minimum.identity() == &member.namespace)
            .ok_or_else(|| error(ErrorKind::Denied, "public-lifecycle-job-roster"))?;
        let mut namespace = git_asset_store::source_claims::admission::Namespace::open(
            &location.root,
            &location.minimum,
        )
        .map_err(admission_error)?;
        if claim.status(&mut namespace)?.receipt.is_none() {
            return Err(error(
                ErrorKind::Denied,
                "public-lifecycle-original-job-unadmitted",
            ));
        }
        let inputs = match &self.configuration.access {
            Access::Active {
                snapshot,
                read_budget,
            } => {
                let mut inputs = GuardedInputs::new(
                    self.configuration.profile.clone(),
                    snapshot,
                    &preview,
                    *read_budget,
                )?;
                inputs.hold_all()?;
                Some(inputs.protect_claim(
                    binding.claim(),
                    self.resources.metadata,
                    &self.resources.limits,
                )?)
            }
            Access::RetainedOnly => None,
        };
        let mut guard = Guard {
            runtime: self,
            binding,
            _controls: controls,
            _inputs: inputs,
            return_controls: None,
            prepared: None,
            retained: None,
            final_controls: None,
        };
        if guard._inputs.is_none() {
            guard.load_return(ticket)?;
            guard.reopen_retained()?;
            guard.hold_final(false)?;
        }
        Ok(guard)
    }
}
/// Deterministic final metadata, built and finished by the caller BEFORE effects.
/// All outputs/receipt/raw return/frozen plans/binding remain independently referenced.
pub fn final_entries(
    binding: &Binding,
    prepared: &Prepared,
) -> Result<BTreeMap<LogicalPath, Object>> {
    let mut entries = BTreeMap::from([
        (path("control/binding")?, binding.pin()),
        (
            path("control/plans")?,
            Object::from_bytes(prepared.control()),
        ),
    ]);
    for (index, artifact) in prepared.candidates().expected().values().enumerate() {
        entries.insert(path(&format!("result/{index:03}"))?, object(artifact)?);
    }
    Ok(entries)
}
/// Explicit immutable metadata preparation only, not native publication/admission.
pub fn final_selection(
    binding: &Binding,
    prepared: &Prepared,
    nodes: &mut impl NodeStore,
) -> Result<git_asset_store::source_claims::admission::Select> {
    binding.retained_seed().selection(
        &final_entries(binding, prepared)?,
        binding.pin().sha256,
        nodes,
    )
}
impl<S: ByteSource> Guard<'_, '_, S> {
    fn ticket(&self, ticket: &Ticket) -> Result<()> {
        if ticket != &self.runtime.configuration.expected {
            Err(error(ErrorKind::Conflict, "public-lifecycle-ticket"))
        } else {
            Ok(())
        }
    }
    fn load_return(&mut self, ticket: &Ticket) -> Result<()> {
        self.ticket(ticket)?;
        if self.prepared.is_some() {
            return Ok(());
        }
        let held = self.binding.result_seed().original(
            &self.runtime.configuration.profile,
            &self.binding.pin().sha256,
            self.runtime.resources.metadata,
            &self.runtime.resources.limits,
            self.runtime.configuration.controls,
        )?;
        if held.entries().len() != 2
            || !held.entries().contains_key(&path("packet")?)
            || !held.entries().contains_key(&path("plans")?)
        {
            return Err(error(
                ErrorKind::Conflict,
                "public-lifecycle-return-controls",
            ));
        }
        let plans = held.entries()[&path("plans")?].clone();
        let prepared = Candidates::bind(
            ticket,
            held.bytes(&path("packet")?)?,
            &self.binding.return_packet_id(),
        )?
        .restore(held.bytes(&path("plans")?)?, &plans)?;
        self.return_controls = Some(held);
        self.prepared = Some(prepared);
        Ok(())
    }
    fn result(&self, result: &ReturnedResult) -> Result<()> {
        if self
            .prepared
            .as_ref()
            .is_none_or(|p| p.candidates().result() != result)
        {
            Err(error(
                ErrorKind::Conflict,
                "public-lifecycle-original-result",
            ))
        } else {
            Ok(())
        }
    }
    fn reopen_retained(&mut self) -> Result<()> {
        let p = self
            .prepared
            .as_ref()
            .ok_or_else(|| error(ErrorKind::NeedsRecovery, "public-lifecycle-return-control"))?;
        let mut held = Vec::new();
        for role in p.plans().keys() {
            held.push(p.reopen(
                role,
                &mut self.runtime.publisher,
                &mut self.runtime.resources,
            )?);
        }
        self.retained = Some(p.gather(held)?);
        Ok(())
    }
    fn hold_final(&mut self, admit: bool) -> Result<()> {
        let p = self
            .prepared
            .as_ref()
            .ok_or_else(|| error(ErrorKind::NeedsRecovery, "public-lifecycle-return-control"))?;
        let request = final_selection(&self.binding, p, &mut MemoryNodes::default())?;
        let c = &self.runtime.configuration;
        let held = if admit {
            self.binding.retained_seed().admit_protection(
                &c.profile,
                &request,
                self.runtime.resources.metadata,
                &self.runtime.resources.limits,
                c.retained,
            )?
        } else {
            self.binding.retained_seed().hold_protection(
                &c.profile,
                &request,
                self.runtime.resources.metadata,
                &self.runtime.resources.limits,
                c.retained,
            )?
        };
        self.final_controls = Some(held);
        Ok(())
    }
    fn publish(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact> {
        self.load_return(ticket)?;
        self.result(result)?;
        if self._inputs.is_none() {
            self.reopen_retained()?;
            self.hold_final(false)?;
        } else {
            let p = self.prepared.as_ref().ok_or_else(|| {
                error(ErrorKind::NeedsRecovery, "public-lifecycle-return-control")
            })?;
            let mut held = Vec::new();
            for role in p.plans().keys() {
                // The SDK reconciles the ORIGINAL one-shot plan before any producer read.
                // Unknown native effect stays Uncertain; no replacement ID/plan is allocated.
                held.push(if role == RETURN_PACKET {
                    p.apply(
                        role,
                        &mut self.runtime.publisher,
                        &mut self.runtime.resources,
                        &mut p.candidates().packet_source(),
                    )?
                } else {
                    p.apply(
                        role,
                        &mut self.runtime.publisher,
                        &mut self.runtime.resources,
                        &mut self.runtime.source,
                    )?
                });
            }
            self.retained = Some(p.gather(held)?);
            self.hold_final(true)?;
        }
        self.retained
            .as_ref()
            .map(|b| b.acknowledgement().clone())
            .ok_or_else(|| error(ErrorKind::NeedsRecovery, "public-lifecycle-retained"))
    }
}
impl<S: ByteSource> ProtectedJob for Guard<'_, '_, S> {
    fn register(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact> {
        self.publish(ticket, result)
    }
    fn reconcile(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact> {
        self.publish(ticket, result)
    }
    fn verify_retained(
        &mut self,
        ticket: &Ticket,
        result: &ReturnedResult,
        ack: &Artifact,
    ) -> Result<()> {
        self.load_return(ticket)?;
        self.result(result)?;
        self.reopen_retained()?;
        self.hold_final(false)?;
        if self
            .retained
            .as_ref()
            .is_none_or(|b| b.acknowledgement() != ack)
        {
            return Err(error(
                ErrorKind::Conflict,
                "public-lifecycle-acknowledgement",
            ));
        }
        Ok(())
    }
}
