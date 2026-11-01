//! Private retained output contracts, never a second editable source inventory.
use super::*;

pub(super) fn workspace_budget(scratch: u64, ticket: u64, ceiling: u64) -> Result<u64> {
    let scratch = scratch_budget(scratch);
    // Entire bounded job.json, the inline ticket and atomic publication of the
    // <=64KiB planning report coexist with the declared compiled-control scratch.
    let required = scratch
        .checked_add(ticket)
        .and_then(|n| n.checked_add(256 * 1024))
        .and_then(|n| n.checked_add(2 * 65_536))
        .ok_or_else(|| anyhow::anyhow!("native planning workspace budget overflow"))?;
    if required > ceiling {
        bail!("native planning workspace allowance exceeds profile ceiling");
    }
    Ok(required)
}
pub(super) fn scratch_budget(configured: u64) -> u64 {
    configured.min(64 * 1024 * 1024)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Budget {
    pub default_max_bytes: u64,
    #[serde(default)]
    pub overrides: BTreeMap<String, u64>,
}
impl Budget {
    pub fn validate(&self) -> Result<()> {
        if self.default_max_bytes == 0 || self.overrides.len() > 511 {
            bail!("invalid native output budget policy");
        }
        for (name, max) in &self.overrides {
            LogicalPath::parse(name)?;
            if *max == 0 {
                bail!("invalid native output override");
            }
        }
        Ok(())
    }
    fn apply(&self, names: &BTreeSet<String>) -> Result<BTreeMap<String, u64>> {
        self.validate()?;
        if names.is_empty()
            || names.len() > 511
            || names.contains("worker.json")
            || self.overrides.keys().any(|key| !names.contains(key))
        {
            bail!("native output policy does not match derived output set");
        }
        Ok(names
            .iter()
            .map(|n| {
                (
                    n.clone(),
                    self.overrides
                        .get(n)
                        .copied()
                        .unwrap_or(self.default_max_bytes),
                )
            })
            .collect())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    schema: String,
    context: Selection,
    recipe: Digest,
    producer: Object,
    contract: crate::scene_delivery::OutputContract,
    pub outputs: BTreeMap<String, u64>,
    atomic_native_output_bytes: u64,
}
fn atomic(outputs: &BTreeMap<String, u64>) -> Result<u64> {
    outputs
        .values()
        .try_fold(0_u64, |sum, n| {
            sum.checked_add(*n)
                .ok_or_else(|| anyhow::anyhow!("native output budget sum overflow"))
        })?
        .checked_mul(2)
        .ok_or_else(|| anyhow::anyhow!("native atomic output budget overflow"))
}
fn names(contract: &crate::scene_delivery::OutputContract) -> BTreeSet<String> {
    contract
        .names
        .iter()
        .cloned()
        .chain(std::iter::once("receipt.json".into()))
        .collect()
}
pub(super) fn produce(
    ticket: &Ticket,
    source: &mut crate::tig_inputs::Source,
    context: &tig_git::source::native::context::ProductionContext,
    programs: &BoundPrograms,
) -> Result<Value> {
    let policy = ticket
        .admission
        .output_budget
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("planning requires output budget policy"))?;
    let contract = if ticket.admission.context_mode == ContextMode::Delivery {
        let prepared =
            crate::tig_delivery_template::lower(source, ticket.admission.request.delivery()?)?;
        let inputs = source.prepare(&prepared.job)?;
        let controls = tig_scene_production::DeliveryControls::new(prepared)?;
        let bound = programs.with_inputs(&inputs);
        let (job, plan) = crate::scene_delivery::plan_with_control_files(
            &controls.job,
            &controls.production,
            &bound,
            &controls,
        )?;
        crate::scene_delivery::output_contract(&job, &plan)?
    } else {
        match ticket.admission.context_mode {
            ContextMode::Declared => {
                tig_scene::compile(
                    source,
                    ticket.admission.request.scene()?,
                    Path::new("compiled"),
                )?;
            }
            ContextMode::Compiled => {
                let recipe = tig_scene::render_recipe(
                    source,
                    ticket.admission.request.scene()?,
                    context,
                    &pin(programs)?,
                    Path::new("compiled"),
                )?;
                if recipe.fingerprint() != &ticket.recipe {
                    bail!("compiled output planning recipe differs");
                }
            }
            ContextMode::Delivery => unreachable!(),
        }
        let path = Path::new("compiled/delivery/job.json").canonicalize()?;
        let job = crate::tig_inputs::read_job(&path)?;
        let inputs = source.prepare(&job)?;
        let bound = programs.with_inputs(&inputs);
        let (job, plan) = crate::scene_delivery::plan_with_inputs(&path, &bound)?;
        crate::scene_delivery::output_contract(&job, &plan)?
    };
    records::check_names(&ticket.admission, &names(&contract))?;
    let outputs = policy.apply(&names(&contract))?;
    let report = Report {
        schema: "reel.native-output-plan.experimental-1".into(),
        context: ticket.admission.context.clone(),
        recipe: ticket.recipe.clone(),
        producer: ticket.producer.clone(),
        contract,
        atomic_native_output_bytes: atomic(&outputs)?,
        outputs,
    };
    let bytes = serde_json::to_vec(&report)?;
    if bytes.len() > 65_536 {
        bail!("native output plan exceeds metadata budget");
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("planning.json")?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(
        json!({"schema":"reel.native-output-planning-complete.experimental-1","output":Object::from_bytes(&bytes),"media_rendered":false,"production_claim":null}),
    )
}
pub(super) fn readback(
    profile: &Profile,
    plan: &JobPlan<'_>,
    programs: &BoundPrograms,
    result: &PhaseResult,
    journal: &mut SourceJournal,
) -> Result<(Report, Checkpoint)> {
    let verified = retained::verify(profile, result, journal)?;
    if result.value["outputs"]
        .as_object()
        .is_none_or(|o| o.len() != 1)
    {
        bail!("planning output set differs");
    }
    let report: Report = serde_json::from_value(identity::parse_json(&verified.bytes(
        result,
        "planning.json",
        65_536,
    )?)?)?;
    let policy = profile
        .worker
        .output_budget
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("planning policy missing"))?;
    if report.schema != "reel.native-output-plan.experimental-1"
        || report.context != profile.worker.context
        || report.recipe != *plan.recipe().fingerprint()
        || report.producer != pin(programs)?
        || report.outputs != policy.apply(&names(&report.contract))?
        || report.atomic_native_output_bytes != atomic(&report.outputs)?
    {
        bail!("native output plan admission differs");
    }
    Ok((report, verified.reader.checkpoint().clone()))
}
#[cfg(test)]
fn owned_selection(
    journal: &mut SourceJournal,
    source: &Key,
    owner: &Key,
    revision: u64,
) -> Result<Selection> {
    retained::owned_selection(journal, source, owner, revision)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn planning_reserves_controls_and_scratch_without_the_full_render_ceiling() {
        let required = workspace_budget(64 * 1024 * 1024, 2048, 256 * 1024 * 1024).unwrap();
        assert_eq!(required, 64 * 1024 * 1024 + 2048 + 256 * 1024 + 2 * 65_536);
        assert!(required < 256 * 1024 * 1024);
        assert_eq!(
            workspace_budget(128 * 1024 * 1024, 2048, 256 * 1024 * 1024).unwrap(),
            required
        );
        assert_eq!(scratch_budget(128 * 1024 * 1024), 64 * 1024 * 1024);
        assert!(workspace_budget(64 * 1024 * 1024, 2048, required - 1).is_err());
        assert!(workspace_budget(64, u64::MAX, u64::MAX).is_err());
    }
    #[test]
    fn budget_policy_rejects_unknown_overrides_zero_caps_large_sets_and_overflow() {
        let names = BTreeSet::from(["D.wav".into(), "receipt.json".into()]);
        let mut policy = Budget {
            default_max_bytes: 8,
            overrides: BTreeMap::from([("receipt.json".into(), 3)]),
        };
        let selected = policy.apply(&names).unwrap();
        assert_eq!(selected["D.wav"], 8);
        assert_eq!(atomic(&selected).unwrap(), 22);
        policy.overrides.insert("unknown.bin".into(), 1);
        assert!(policy.apply(&names).is_err());
        policy.overrides.clear();
        policy.default_max_bytes = 0;
        assert!(policy.apply(&names).is_err());
        policy.default_max_bytes = 1;
        policy.overrides.insert("D.wav".into(), 0);
        assert!(policy.apply(&names).is_err());
        policy.overrides.clear();
        assert!(
            policy
                .apply(&(0..512).map(|n| format!("frame-{n:03}.png")).collect())
                .is_err()
        );
        assert!(atomic(&BTreeMap::from([("a".into(), u64::MAX)])).is_err());
        assert!(atomic(&BTreeMap::from([("a".into(), u64::MAX), ("b".into(), 1)])).is_err());
    }
    #[test]
    fn owned_result_refuses_foreign_stale_pending_and_aborted_versions() {
        use tig_store::source_claims::Command;
        let root = tempfile::tempdir().unwrap();
        let key = |s: &str| Key::parse(s).unwrap();
        let source = key("source");
        let owner = key("owner");
        let mut journal =
            SourceJournal::create(&root.path().join("journal"), key("fixture"), key("epoch"))
                .unwrap();
        let selection = Selection {
            root: Object::from_bytes(b"root"),
            context: Digest::blob(b"context"),
        };
        let prepared = journal
            .apply(
                key("prepare"),
                Command::Prepare {
                    source: source.clone(),
                    owner: owner.clone(),
                    expected_revision: 0,
                    selection: selection.clone(),
                },
            )
            .unwrap();
        journal
            .apply(
                key("adopt"),
                Command::Adopt {
                    source: source.clone(),
                    owner: owner.clone(),
                    expected_revision: 1,
                    prepared: prepared.reference,
                },
            )
            .unwrap();
        let before = journal.checkpoint();
        assert_eq!(
            owned_selection(&mut journal, &source, &owner, 2).unwrap(),
            selection
        );
        assert!(owned_selection(&mut journal, &source, &key("foreign"), 2).is_err());
        assert!(owned_selection(&mut journal, &source, &owner, 1).is_err());
        assert_eq!(journal.checkpoint(), before);
        let pending = journal
            .apply(
                key("next"),
                Command::Prepare {
                    source: source.clone(),
                    owner: owner.clone(),
                    expected_revision: 2,
                    selection,
                },
            )
            .unwrap();
        assert!(owned_selection(&mut journal, &source, &owner, 3).is_err());
        journal
            .apply(
                key("abort"),
                Command::Abort {
                    source: source.clone(),
                    owner: owner.clone(),
                    expected_revision: 3,
                    prepared: pending.reference,
                },
            )
            .unwrap();
        assert!(owned_selection(&mut journal, &source, &owner, 4).is_err());
    }
}
