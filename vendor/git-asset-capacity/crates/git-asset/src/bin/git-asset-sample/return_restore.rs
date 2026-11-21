//! Actual missing -> retained backup -> working restore in one returned source.
use super::*;
struct Clock(Digest, u64);
impl final_provider::Clock for Clock {
    fn now(&self, policy: &Digest) -> final_provider::Result<u64> {
        if policy != &self.0 {
            return Err(final_provider::Error::Clock);
        }
        Ok(self.1)
    }
}
fn snapshot(d: &Demo, id: &str) -> Result<Value> {
    let actual = d.copy_snapshot()?;
    let stage = d.fixture["stages"]
        .as_array()
        .ok_or("missing stages")?
        .iter()
        .find(|s| s["id"] == id)
        .ok_or("missing joined oracle")?;
    for name in d.objects.keys() {
        let observed = ["working", "history", "backup"]
            .into_iter()
            .filter(|r| actual[*r].get(name).is_some())
            .collect::<BTreeSet<_>>();
        let expected = stage["expected_copies"][name]
            .as_array()
            .ok_or("missing joined copies")?
            .iter()
            .map(|v| v.as_str().ok_or("invalid joined role"))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        if observed != expected {
            return Err(
                format!("{id}/{name}: expected {expected:?}, observed {observed:?}").into(),
            );
        }
    }
    Ok(actual)
}
fn guard(d: &Demo) -> Result<()> {
    let at = d
        .config
        .providers
        .iter()
        .position(|p| p.assignments[0].place.as_str() == "history")
        .ok_or("missing history")?;
    if !matches!(
        placement::publication(
            &d.config,
            &d.config.providers[at],
            &d.stores[at],
            &d.objects["original"]
        ),
        Err(placement::Error::FinalRetirement)
    ) {
        return Err("joined restore replaced final history birth".into());
    }
    Ok(())
}
fn record(d: &mut Demo, id: &str, observation: Value) -> Result<Object> {
    let file = format!("reports/{id}.json");
    let object = save(&d.root.join(&file), &observation)?;
    fs::copy(
        d.root.join(&file),
        Layout::Flat.object_path(&d.root.join("lesson-evidence"), &object.sha256),
    )?;
    let stage = d.fixture["stages"]
        .as_array()
        .ok_or("missing stages")?
        .iter()
        .find(|s| s["id"] == id)
        .ok_or("missing joined stage")?;
    d.reports.push(json!({"lesson":id,"report":object,"file":file,"title":id.replace('-'," "),"event":stage["event"]}));
    Ok(object)
}
pub(super) fn run(d: &mut Demo, commit: &str) -> Result<Value> {
    guard(d)?;
    if !d.seed.disabled {
        return Err("joined restore seed must remain disabled".into());
    }
    let missing = snapshot(d, "git-return-after-final-loss")?;
    let source = d.namespaces[0].checkpoint();
    let seed_attempts = d.seed.attempts;
    let final_report = d
        .reports
        .iter()
        .find(|r| r["lesson"] == "final-loss-completed")
        .ok_or("missing final parent")?["report"]
        .clone();
    let missing_report = d.reports.last().ok_or("missing returned lesson")?["report"].clone();
    let loss: Value = serde_json::from_slice(&fs::read(d.root.join("final-loss.json"))?)?;
    let old = final_bulk::Plan::from_json(&identity::canonical_json(&loss["plan"])?)?;
    let at = d
        .config
        .providers
        .iter()
        .position(|p| p.assignments[0].place.as_str() == "backup")
        .ok_or("missing backup")?;
    let parent = old
        .actions()
        .find(|(store, p)| {
            *store == d.stores[at].identity()
                && p.request().publication.object == d.objects["original"]
        })
        .ok_or("missing final backup action")?
        .1;
    let request = git_asset_store::Request {
        operation: key("returned-original-new-backup"),
        ..parent.request().publication.clone()
    };
    let context = Digest::blob(b"public returned-source explicit acquisition");
    let plan = final_provider::reacquire::plan(
        &d.stores[at],
        parent,
        final_provider::reacquire::Request {
            publication: request.clone(),
            capacity: d.ledgers[at].identity().clone(),
            context: context.clone(),
            window: final_provider::Window {
                not_before: 50,
                expires: 500,
            },
        },
    )?;
    let clock = Clock(old.enrollment().clock.clone(), 100);
    let bytes = d.fixture["objects"]["original"]["text"]
        .as_str()
        .ok_or("missing public original")?
        .as_bytes()
        .to_vec();
    let mut input = std::io::Cursor::new(bytes);
    let receipt = final_provider::reacquire::Execution {
        store: &d.stores[at],
        ledger: &mut d.ledgers[at],
        job: &d.config.providers[at].job,
        clock: &clock,
        admitted_context: &context,
    }
    .apply(&plan, &plan.authorize()?, &mut input)?;
    if input.position() != d.objects["original"].bytes {
        return Err("joined acquisition did not stream exact original".into());
    }
    let publication = reacquisition_playback::publication(&receipt, input.position())?;
    let retained = git_asset_store::retention::plan(
        &d.stores[at],
        &git_asset_store::retention::RetentionRequest {
            operation: request.operation.clone(),
            owner: key("sample"),
            publication: request,
            minimum_ack: git_asset_store::AckLevel::ProcessRestart,
        },
    )?;
    let grant = git_asset_store::retention::retain(
        &mut d.ledgers[at],
        &d.stores[at],
        &d.config.providers[at].job,
        &retained,
    )?
    .grant;
    git_asset_store::retention::verify(&mut d.ledgers[at], &d.stores[at], &grant)?;
    let acquisition = json!({"plan":serde_json::from_slice::<Value>(&plan.to_json()?)?,"retention":grant,"publication":publication});
    let acquired = snapshot(d, "returned-backup-acquired")?;
    guard(d)?;
    let (placement_plan, status) = d.with_placement(|e| {
        let p = e.plan(None)?;
        let s = e.status(&p)?;
        Ok((p, s))
    })?;
    if status.storage_converged != Some(false)
        || d.namespaces[0].checkpoint() != source
        || d.seed.attempts != seed_attempts
    {
        return Err("backup-only acquisition falsely restored working/source".into());
    }
    let common = json!({"schema":"tig.teaching-return-restore-observation.design-1","source_commit":commit,"source_checkpoint":source,
        "providers":d.config.providers.iter().map(|p|&p.place.instance).collect::<Vec<_>>(),"old_bulk_plan":serde_json::from_slice::<Value>(&old.to_json()?)?,
        "final_parent_report":final_report,"missing_report":missing_report,"acquisition":acquisition,"acquisition_time":100,
        "seed_disabled":true,"seed_attempts_before":seed_attempts,"seed_attempts_after":d.seed.attempts,"history_final_guard":true,
        "synthetic_owner_consent":true,"simulated_clock":true,"retention_verified_at_capture":true,"independent_hardware_backup_proven":false,"authorizes_storage":false});
    let mut observation = common.clone();
    observation["lesson"] = "returned-backup-acquired".into();
    observation["phase"] = "backup_acquired".into();
    observation["previous_report"] = missing_report;
    observation["before"] = missing;
    observation["after"] = acquired.clone();
    observation["placement_plan"] = serde_json::to_value(&placement_plan)?;
    observation["placement_apply"] = Value::Null;
    observation["placement_status"] = serde_json::to_value(&status)?;
    observation["replay"] = Value::Null;
    observation["before_replay"] = Value::Null;
    observation["after_replay"] = Value::Null;
    observation["retry_time"] = Value::Null;
    let acquired_report = record(d, "returned-backup-acquired", observation)?;
    let (p, applied, status) = d.with_placement(|e| {
        let p = e.plan(None)?;
        let a = e.apply(&p)?;
        let s = e.status(&p)?;
        Ok((p, a, s))
    })?;
    if !applied.page_converged
        || applied.storage_converged != Some(true)
        || !applied.holds.is_empty()
        || applied.stream_bytes != 29
        || !status.page_converged
        || status.storage_converged != Some(true)
        || d.seed.attempts != seed_attempts
    {
        return Err(format!("joined working restore incomplete: {:?}", applied.holds).into());
    }
    let restored = snapshot(d, "returned-working-restored")?;
    guard(d)?;
    let minimum: git_asset_store::ledger::Checkpoint =
        serde_json::from_value(loss["witness_checkpoint"].clone())?;
    let preview: Value = serde_json::from_slice(&fs::read(d.root.join("final-preview.json"))?)?;
    let witness_job = serde_json::from_value(preview["witness_job"].clone())?;
    let witness = LocalStore::open(&d.root.join("final-witness"))?;
    let mut ledger = CapacityLedger::open(&d.root.join("final-witness-ledger"), &minimum)?;
    let before = json!({"source":d.namespaces[0].checkpoint(),"ledgers":d.ledgers.iter().map(CapacityLedger::checkpoint).collect::<Vec<_>>(),"witness":ledger.checkpoint()});
    let jobs = old
        .actions()
        .map(|(store, _)| {
            d.config
                .providers
                .iter()
                .find(|p| &p.place.instance == store)
                .map(|p| p.job.clone())
                .ok_or("unknown old action")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let roster = d.final_roster.clone().ok_or("missing final roster")?;
    let expired = Clock(old.enrollment().clock.clone(), 700);
    let replay = {
        let mut places = d
            .stores
            .iter()
            .zip(&mut d.ledgers)
            .zip(&d.config.providers)
            .map(|((store, ledger), p)| placement::Local {
                store,
                ledger,
                job: &p.job,
            })
            .collect::<Vec<_>>();
        final_bulk::Execution {
            enrollment: old.enrollment(),
            roster: &roster,
            namespaces: &mut d.namespaces,
            metadata: &mut d.metadata,
            places: &mut places,
            action_jobs: &jobs,
            witness: placement::Local {
                store: &witness,
                ledger: &mut ledger,
                job: &witness_job,
            },
            clock: &expired,
            admitted_context: &old.enrollment().context,
        }
        .apply(&old, &old.authorize()?)?
    };
    let after = json!({"source":d.namespaces[0].checkpoint(),"ledgers":d.ledgers.iter().map(CapacityLedger::checkpoint).collect::<Vec<_>>(),"witness":ledger.checkpoint()});
    if before != after
        || replay.phase != final_bulk::Phase::Completed
        || replay.new_logical_copy_bytes_removed != 0
        || replay.historical_deleted_copy_bytes != 58
        || !replay.holds.is_empty()
        || replay.removal_authority
        || d.copy_snapshot()? != restored
        || git(&d.git, &d.root.join("project"), &["rev-parse", "HEAD"])? != commit
    {
        return Err("old deletion retry changed returned-source restoration".into());
    }
    git_asset_store::retention::verify(&mut d.ledgers[at], &d.stores[at], &grant)?;
    guard(d)?;
    let mut observation = common;
    observation["lesson"] = "returned-working-restored".into();
    observation["phase"] = "working_restored".into();
    observation["previous_report"] = serde_json::to_value(&acquired_report)?;
    observation["before"] = acquired;
    observation["after"] = restored;
    observation["placement_plan"] = serde_json::to_value(&p)?;
    observation["placement_apply"] = serde_json::to_value(&applied)?;
    observation["placement_status"] = serde_json::to_value(&status)?;
    let full_replay = save(&d.root.join("returned-old-retry.json"), &replay)?;
    observation["replay"] = json!({"plan":replay.plan,"phase":replay.phase,"intent_acknowledged":replay.intent_acknowledged,
        "selected_roots":replay.selected_roots,"selected_bindings":replay.selected_bindings,"unique_objects":replay.unique_objects,
        "planned_copy_bytes":replay.planned_copy_bytes,"new_removed_bytes":replay.new_logical_copy_bytes_removed,
        "historical_removed_bytes":replay.historical_deleted_copy_bytes,"holds":replay.holds,"removal_authority":replay.removal_authority});
    observation["before_replay"] = before;
    observation["after_replay"] = after;
    observation["retry_time"] = 700.into();
    let restored_report = record(d, "returned-working-restored", observation)?;
    Ok(json!({"acquired":acquired_report,"restored":restored_report,"old_retry":full_replay}))
}
