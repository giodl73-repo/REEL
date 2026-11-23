//! Ordered controller over original native admission and guarded execution ports.
use super::{
    bookkeeping, claim, codec, factories, history,
    model::{self, Handle, Recipe},
    projection, removal,
    session::Session,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tig_core::{
    Digest, LogicalPath, Object,
    tree::{Tree, TreeLimits},
};
use tig_store::{
    ledger,
    retention::RetentionGrant,
    retirement_intent::{
        self as intent, ObservedIntent,
        progress::{self, Action, Admission, handoff_work, local_work, release_work},
    },
};

fn merge_capacity(
    minimums: &mut BTreeMap<Digest, ledger::Checkpoint>,
    id: &Digest,
    incoming: ledger::Checkpoint,
) -> Result<(), String> {
    let current = minimums
        .get(id)
        .ok_or("unknown result capacity authority")?;
    let old = serde_json::to_value(current).map_err(|_| "invalid capacity witness")?;
    let new = serde_json::to_value(&incoming).map_err(|_| "invalid result capacity witness")?;
    if old["genesis"] != new["genesis"]
        || (current.sequence() == incoming.sequence() && current != &incoming)
    {
        return Err("conflicting result capacity witness".into());
    }
    if incoming.sequence() > current.sequence() {
        minimums.insert(id.clone(), incoming);
    }
    Ok(())
}
fn grants(
    session: &mut Session,
    recipe: &Recipe,
) -> Result<BTreeMap<Digest, RetentionGrant>, String> {
    let (entries, _) = Tree::from_root(recipe.grants.clone())
        .entries(
            &mut session.nodes,
            TreeLimits {
                max_entries: 256,
                ..TreeLimits::default()
            },
        )
        .map_err(|_| "saved grant index unavailable")?;
    let mut total = recipe
        .review
        .bytes
        .checked_add(recipe.context.bytes)
        .ok_or("control budget")?;
    let mut result = BTreeMap::new();
    for (path, object) in entries {
        total = total.checked_add(object.bytes).ok_or("control budget")?;
        if total > model::MAX_CONTROL_BYTES {
            return Err("saved control inventory exceeds budget".into());
        }
        let grant = RetentionGrant::from_json(&codec::get(&session.nodes, &object)?)
            .map_err(|_| "invalid saved native grant")?;
        if path.as_str() != format!("claims/{}", grant.claim_id().as_str())
            || result.insert(grant.claim_id().clone(), grant).is_some()
        {
            return Err("saved native grant index binding changed".into());
        }
    }
    Ok(result)
}
struct Runner {
    session: Option<Session>,
    recipe: Recipe,
    handle: Handle,
    ctx: ObservedIntent,
    policy_authority: Option<std::sync::Arc<intent::policy::NativeAuthority>>,
    grants: BTreeMap<Digest, RetentionGrant>,
    receipts: BTreeMap<LogicalPath, Object>,
    completed: u32,
    historical: u32,
    pending: Option<String>,
    maximum_new_actions: u32,
}
impl Runner {
    fn capacity(&self, place: &Digest) -> Result<Value, String> {
        let capacity = &self
            .recipe
            .places
            .get(place)
            .ok_or("missing exact native place")?
            .capacity;
        Ok(
            json!({"root":capacity.root,"ledger":capacity.ledger,"minimum_checkpoint":self.handle.capacity_minimums.get(&capacity.ledger).ok_or("missing current capacity minimum")?}),
        )
    }
    fn native(&self, place: &Digest) -> Result<Value, String> {
        Ok(
            json!({"root":self.recipe.places.get(place).ok_or("missing native place binding")?.root,"place":place,"capacity":self.capacity(place)?}),
        )
    }
    fn witnesses(&mut self) -> Result<(), String> {
        let session = self
            .session
            .as_mut()
            .ok_or("controller authority session unavailable")?;
        session
            .nodes
            .finish()
            .map_err(|_| "controller metadata finish uncertain")?;
        self.handle.authorities = session.witnesses(&self.handle.authorities);
        if let Some(current) = self
            .policy_authority
            .as_ref()
            .and_then(|a| a.registry_checkpoint())
        {
            let old = self
                .handle
                .policy_minimum
                .as_ref()
                .ok_or("missing policy work witness")?;
            if current.sequence() == old.sequence() && &current != old {
                return Err("conflicting policy witnesses".into());
            }
            if current.sequence() > old.sequence() {
                self.handle.policy_minimum = Some(current);
            }
        }
        Ok(())
    }
    fn execute(
        &mut self,
        input: &Value,
        is_removal: bool,
        source: &Digest,
        successor: Option<&Digest>,
    ) -> Result<(Value, u8), String> {
        self.witnesses()?;
        drop(self.session.take());
        let result = if is_removal {
            removal::apply(input, self.policy_authority.clone())
        } else {
            claim::apply(input, self.policy_authority.clone())
        };
        // Refresh original authorities after every command boundary. An error
        // may follow effects; reopen known minima and stop, never re-plan.
        let mut required = self.handle.authorities.clone();
        if let Ok((out, _)) = &result {
            let checkpoints = &out["last_confirmed_minimum_checkpoints"];
            if !checkpoints["index"].is_null() {
                required.index.minimum_checkpoint =
                    serde_json::from_value(checkpoints["index"].clone())
                        .map_err(|_| "invalid native result index witness")?;
            }
            required.journal.minimum_checkpoint =
                serde_json::from_value(checkpoints["journal"].clone())
                    .map_err(|_| "invalid native result journal witness")?;
            required.metadata.minimum_checkpoint =
                serde_json::from_value(checkpoints["metadata"].clone())
                    .map_err(|_| "invalid native result metadata witness")?;
            let field = if is_removal {
                "capacity"
            } else {
                "source_capacity"
            };
            if !checkpoints[field].is_null() {
                let id = &self
                    .recipe
                    .places
                    .get(source)
                    .ok_or("missing source capacity")?
                    .capacity
                    .ledger;
                merge_capacity(
                    &mut self.handle.capacity_minimums,
                    id,
                    serde_json::from_value(checkpoints[field].clone())
                        .map_err(|_| "invalid native capacity result")?,
                )?;
            }
            if let Some(place) = successor.filter(|_| !checkpoints["successor_capacity"].is_null())
            {
                let id = &self
                    .recipe
                    .places
                    .get(place)
                    .ok_or("missing successor capacity")?
                    .capacity
                    .ledger;
                merge_capacity(
                    &mut self.handle.capacity_minimums,
                    id,
                    serde_json::from_value(checkpoints["successor_capacity"].clone())
                        .map_err(|_| "invalid successor capacity result")?,
                )?;
            }
            if is_removal {
                for survivor in out["survivors_opened"]
                    .as_array()
                    .ok_or("invalid native survivor witnesses")?
                {
                    let id: Digest = serde_json::from_value(survivor["ledger"].clone())
                        .map_err(|_| "invalid survivor ledger identity")?;
                    merge_capacity(
                        &mut self.handle.capacity_minimums,
                        &id,
                        serde_json::from_value(survivor["minimum_checkpoint"].clone())
                            .map_err(|_| "invalid survivor capacity result")?,
                    )?;
                }
            }
        }
        self.session = Some(Session::reconcile(&required, &self.handle.authorities)?);
        self.witnesses()?;
        result
    }
    fn record(&mut self, result: Value, complete: bool, historical: bool) -> Result<(), String> {
        if self.receipts.len() >= self.recipe.limits.maximum_actions as usize {
            return Err("controller receipt action bound".into());
        }
        let session = self
            .session
            .as_mut()
            .ok_or("controller session unavailable after native effect")?;
        let object = codec::put(&mut session.nodes, &codec::encode(&result)?)?;
        let path = LogicalPath::parse(&format!("actions/{:03}", self.receipts.len()))
            .map_err(|_| "invalid receipt path")?;
        self.receipts.insert(path, object);
        if complete {
            self.completed += 1;
        }
        if historical {
            self.historical += 1;
        }
        Ok(())
    }
    fn claim(&mut self, release: &tig_core::retirement::review::Release) -> Result<bool, String> {
        let session = self.session.as_mut().ok_or("controller session missing")?;
        let snapshot = progress::inspect(
            &mut session.journal,
            &session.nodes,
            &self.ctx,
            &release.object,
        )
        .map_err(|_| "native claim progress unavailable")?;
        if snapshot.released.contains(&release.claim) {
            self.record(json!({"kind":"claim","claim":release.claim,"object":release.object,"claim_work_complete":true,"reported_completion_reused":true,
                "current_claim_state":"unknown","current_custody":"unknown","payload_deleted":false}),true,true)?;
            return Ok(true);
        }
        if self.completed - self.historical >= self.maximum_new_actions {
            return Err("invocation action limit reached; resume exact work".into());
        }
        let handoff = self
            .recipe
            .handoffs
            .iter()
            .find(|h| h.claim == release.claim)
            .cloned();
        let grant = self
            .grants
            .get(&release.claim)
            .ok_or("missing selected native grant")?
            .clone();
        let expected = Action::Release {
            claim: release.claim.clone(),
        };
        let attempt = if let Some((attempt, action)) = snapshot.in_flight {
            if action != expected {
                return Err("another exact action remains in flight".into());
            }
            attempt
        } else {
            if snapshot.cancelled {
                return Err("cancelled intent stops new claim admission".into());
            }
            let mut cause = None;
            let admitted: Result<Admission, intent::Error> = if let Some(binding) = &handoff {
                let successor = self
                    .grants
                    .get(&binding.successor)
                    .ok_or("missing exact successor grant")?;
                handoff_work::start(
                    &mut session.journal,
                    &mut session.nodes,
                    &self.ctx,
                    &release.object,
                    &release.claim,
                    |attempt| {
                        factories::handoff(
                            &self.recipe,
                            &self.ctx,
                            &grant,
                            successor,
                            binding,
                            attempt,
                            &mut self.handle.capacity_minimums,
                        )
                        .map_err(|error| {
                            cause = Some(error);
                            intent::Error::Invalid
                        })
                    },
                )
            } else {
                release_work::start(
                    &mut session.journal,
                    &mut session.nodes,
                    &self.ctx,
                    &release.object,
                    &release.claim,
                    |attempt| {
                        factories::release(
                            &self.recipe,
                            &self.ctx,
                            &grant,
                            attempt,
                            &mut self.handle.capacity_minimums,
                        )
                        .map_err(|error| {
                            cause = Some(error);
                            intent::Error::Invalid
                        })
                    },
                )
            };
            admitted
                .map_err(|e| {
                    cause.unwrap_or_else(|| format!("native claim admission pending ({e:?})"))
                })?
                .attempt
        };
        self.witnesses()?;
        let successor = handoff
            .as_ref()
            .map(|h| self.grants[&h.successor].store().clone());
        let mode = if let Some(place) = &successor {
            json!({"kind":"handoff","successor":self.native(place)?})
        } else {
            json!({"kind":"retirement"})
        };
        let input = json!({"schema":"tig.claim-execution-profile.experimental-2","enrollment":self.recipe.enrollment,
            "operation":self.recipe.operation,"decision":self.ctx.decision().digest().map_err(|_| "invalid decision")?,
            "object":release.object,"attempt":attempt,"claim":release.claim,"source":self.native(grant.store())?,"mode":mode,
            "journal":self.handle.authorities.journal,"metadata":self.handle.authorities.metadata,
            "maximum_object_bytes":self.recipe.limits.maximum_object_bytes,"allow_cooperative":self.recipe.limits.allow_cooperative,
            "intent_execution_profile":self.ctx.execution_profile().ok_or("missing execution context")?.fingerprint().map_err(|_| "invalid execution context")?});
        let (result, code) = self.execute(&input, false, grant.store(), successor.as_ref())?;
        let complete = code == 0 && result["claim_work_complete"] == true;
        let historical = result["reported_completion_reused"] == true;
        if !complete {
            self.pending = Some(
                result["pending"]
                    .as_str()
                    .unwrap_or("native claim work remains pending")
                    .into(),
            );
        }
        self.record(result, complete, historical)?;
        Ok(complete)
    }
    fn remove(
        &mut self,
        target: &tig_core::retirement::review::decision::Target,
    ) -> Result<bool, String> {
        let session = self.session.as_mut().ok_or("controller session missing")?;
        let snapshot = progress::inspect(
            &mut session.journal,
            &session.nodes,
            &self.ctx,
            &target.object,
        )
        .map_err(|_| "native removal progress unavailable")?;
        if snapshot.removed.contains_key(&target.store) {
            self.record(json!({"kind":"removal","object":target.object,"place":target.store,"removal_work_complete":true,
                "reported_completion_reused":true,"current_absence":"unknown","current_custody":"unknown","net_reclaimed_bytes":null}),true,true)?;
            return Ok(true);
        }
        if self.completed - self.historical >= self.maximum_new_actions {
            return Err("invocation action limit reached; resume exact work".into());
        }
        let attempt = if let Some((attempt, action)) = snapshot.in_flight {
            if action
                != (Action::Remove {
                    store: target.store.clone(),
                })
            {
                return Err("another exact action remains in flight".into());
            }
            attempt
        } else {
            if snapshot.cancelled {
                return Err("cancelled intent stops new removal admission".into());
            }
            let mut cause = None;
            local_work::start(
                &mut session.journal,
                &mut session.nodes,
                &self.ctx,
                &target.object,
                &target.store,
                |attempt| {
                    factories::removal(
                        &self.recipe,
                        &self.ctx,
                        &target.object,
                        &target.store,
                        attempt,
                        &mut self.handle.capacity_minimums,
                    )
                    .map_err(|error| {
                        cause = Some(error);
                        intent::Error::Invalid
                    })
                },
            )
            .map_err(|e| {
                cause.unwrap_or_else(|| format!("native removal admission pending ({e:?})"))
            })?
            .attempt
        };
        self.witnesses()?;
        let preservation = self
            .recipe
            .preservations
            .iter()
            .find(|p| p.source == target.store && p.object == target.object)
            .map(|p| &p.profile);
        let survivors = preservation.map(|profile| profile.retained.iter().map(|s| {
            let place = &s.commitment.place;
            Ok(json!({"destination":{"root":self.recipe.places.get(place).ok_or("missing survivor binding")?.root,"place":place},
                "capacity":self.capacity(place)?,"grant":self.grants.get(&s.commitment.claim).ok_or("missing survivor grant")?}))
        }).collect::<Result<Vec<Value>,String>>()).transpose()?.unwrap_or_default();
        let input = json!({"schema":"tig.removal-execution-profile.experimental-3","enrollment":self.recipe.enrollment,
            "operation":self.recipe.operation,"decision":self.ctx.decision().digest().map_err(|_| "invalid decision")?,"object":target.object,"attempt":attempt,
            "destination":{"root":self.recipe.places.get(&target.store).ok_or("missing removal binding")?.root,"place":target.store},"capacity":self.capacity(&target.store)?,
            "index":self.handle.authorities.index,"journal":self.handle.authorities.journal,"metadata":self.handle.authorities.metadata,
            "preservation":preservation,"survivors":survivors,"maximum_object_bytes":self.recipe.limits.maximum_object_bytes,
            "allow_cooperative":self.recipe.limits.allow_cooperative,"intent_execution_profile":self.ctx.execution_profile().ok_or("missing context")?.fingerprint().map_err(|_| "invalid context")?});
        let (result, code) = self.execute(&input, true, &target.store, None)?;
        let complete = code == 0 && result["removal_work_complete"] == true;
        let historical = result["reported_completion_reused"] == true;
        if !complete {
            self.pending = Some(
                result["pending"]
                    .as_str()
                    .unwrap_or("native removal work remains pending")
                    .into(),
            );
        }
        self.record(result, complete, historical)?;
        Ok(complete)
    }
    fn finish(mut self) -> Result<(Vec<u8>, u8), String> {
        self.witnesses()?;
        let session = self
            .session
            .as_mut()
            .ok_or("native effect may exist; reopen exact work handle")?;
        let (tree, _) = Tree::build(&self.receipts, &mut session.nodes, TreeLimits::default())
            .map_err(|_| "controller receipt index pending")?;
        session
            .nodes
            .finish()
            .map_err(|_| "controller receipt acknowledgement uncertain; effects may exist")?;
        self.handle.authorities = session.witnesses(&self.handle.authorities);
        self.handle.digest = codec::handle_digest(&self.handle)?;
        let complete = self.pending.is_none();
        Ok((
            codec::encode(
                &json!({"schema":"tig.native-retirement-result.experimental-1","work":self.handle,
            "mode":self.recipe.mode,"work_complete":complete,"completed_actions":self.completed,"historical_actions":self.historical,
            "receipts":tree.root(),"pending":self.pending,"current_custody":"unknown","current_absence":"unknown","net_reclaimed_bytes":null,
            "enforcement":"cooperative-process-restart"}),
            )?,
            if complete { 0 } else { 2 },
        ))
    }
}
pub fn apply(bytes: &[u8]) -> Result<(Vec<u8>, u8), String> {
    apply_with_limit(bytes, 256)
}
pub fn apply_with_limit(bytes: &[u8], maximum_new_actions: u32) -> Result<(Vec<u8>, u8), String> {
    if !(1..=256).contains(&maximum_new_actions) {
        return Err("invocation action limit must be 1..256".into());
    }
    let mut handle = codec::handle(bytes)?;
    let mut session = Session::open(&handle.authorities)?;
    let recipe = bookkeeping::load(&mut session.journal, &session.nodes, &handle)?;
    let ctx = intent::inspect_retained(
        &mut session.journal,
        &session.nodes,
        &Session::enrollment(&handle.enrollment),
        &handle.operation,
    )
    .map_err(|_| "saved original retirement intent unavailable")?;
    if Object::from_bytes(
        &ctx.decision()
            .to_json()
            .map_err(|_| "invalid saved decision")?,
    ) != recipe.decision
        || Object::from_bytes(&ctx.review().to_json().map_err(|_| "invalid saved review")?)
            != recipe.review
        || Object::from_bytes(
            &ctx.execution_profile()
                .ok_or("missing saved execution context")?
                .to_json()
                .map_err(|_| "invalid execution context")?,
        ) != recipe.context
    {
        return Err("recipe differs from exact original intent".into());
    }
    if recipe.policy_authority.is_some() != handle.policy_minimum.is_some() {
        return Err("missing/foreign policy work witness".into());
    }
    let policy_authority = recipe
        .policy_authority
        .clone()
        .map(intent::policy::NativeAuthority::new)
        .transpose()
        .map_err(|_| "invalid saved policy authority")?
        .map(|a| {
            std::sync::Arc::new(
                a.require_registry_minimum(
                    handle
                        .policy_minimum
                        .clone()
                        .expect("policy handle checked"),
                ),
            )
        });
    let ctx = if let Some(authority) = &policy_authority {
        ctx.attach_policy_authority(&session.nodes, authority.clone())
            .map_err(|_| "policy authority/context mismatch")?
    } else {
        ctx
    };
    if ctx
        .execution_profile()
        .is_some_and(|p| p.policy_proof().is_some())
        != policy_authority.is_some()
    {
        return Err("missing/foreign saved policy authority".into());
    }
    super::admission::review_mode(recipe.mode, ctx.review())?;
    let expected = model::capacity_minimums(recipe.places.values())?;
    if expected.keys().ne(handle.capacity_minimums.keys()) {
        return Err("work capacity authority set differs from recipe".into());
    }
    for (id, checkpoint) in expected {
        merge_capacity(&mut handle.capacity_minimums, &id, checkpoint)?;
    }
    let grants = grants(&mut session, &recipe)?;
    let mut runner = Runner {
        session: Some(session),
        recipe,
        handle,
        ctx,
        policy_authority,
        grants,
        receipts: BTreeMap::new(),
        completed: 0,
        historical: 0,
        pending: None,
        maximum_new_actions,
    };
    let ordered = (|| {
        let session = runner
            .session
            .as_mut()
            .ok_or("missing controller session")?;
        history::apply(session, &runner.recipe, &runner.handle.recipe, &runner.ctx)?;
        projection::apply(session, &runner.recipe, &runner.handle.recipe, &runner.ctx)?;
        let consent = runner.ctx.decision().consent().clone();
        for release in &consent.release_owned_claims {
            if runner
                .ctx
                .execution_profile()
                .ok_or("missing saved context")?
                .history_releases()
                .contains(&release.claim)
            {
                continue;
            }
            if !runner.claim(release)? {
                return Ok(());
            }
        }
        for target in &consent.remove_copies {
            if !runner.remove(target)? {
                return Ok(());
            }
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = ordered {
        runner.pending = Some(error);
    }
    runner.finish()
}
