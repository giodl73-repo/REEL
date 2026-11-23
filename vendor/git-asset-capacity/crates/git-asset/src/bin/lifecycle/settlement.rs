//! Explicit source-settlement operator interface. No hook or provider initialization.
use super::profiles::Storage;
use super::*;
use git_asset::lifecycle::settlement;
use git_asset_store::placement::Cursor;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettlementProfile {
    schema: String,
    enrollment: settlement::Enrollment,
    git_executable: PathBuf,
    repository_root: PathBuf,
    source_limits: Limits,
    namespace_root: PathBuf,
    namespace_minimum: source_claims::Checkpoint,
    metadata_root: PathBuf,
    metadata_minimum: metadata::Checkpoint,
    #[serde(default)]
    allow_withdraw: bool,
}
pub(super) fn run(args: &[std::ffi::OsString]) -> Result<Value, &'static str> {
    if args.len() < 3 {
        return Err("usage");
    }
    let action = args[1].to_str().ok_or("usage")?;
    if !matches!(action, "plan" | "status" | "apply" | "resume" | "reconcile")
        || (action == "plan" && args.len() != 3)
        || (matches!(action, "status" | "apply" | "resume") && args.len() != 4)
        || (action == "reconcile" && !(4..=5).contains(&args.len()))
    {
        return Err("usage");
    }
    let profile: SettlementProfile = parse(&read(Path::new(&args[2]))?)?;
    if profile.schema != "tig.source-settlement-profile.design-1" {
        return Err("invalid_document");
    }
    profile.enrollment.id().map_err(code)?;
    for path in [
        &profile.git_executable,
        &profile.repository_root,
        &profile.namespace_root,
        &profile.metadata_root,
    ] {
        absolute(path)?;
    }
    let mut namespace = Namespace::open(&profile.namespace_root, &profile.namespace_minimum)
        .map_err(|_| "journal_unavailable")?;
    let existing = if matches!(action, "status" | "apply" | "resume") {
        Some(settlement::Plan::from_json(&read(Path::new(&args[3]))?).map_err(code)?)
    } else {
        None
    };
    if action == "status" {
        return serde_json::to_value(
            settlement::inspect(
                &profile.enrollment,
                &mut namespace,
                existing.as_ref().ok_or("usage")?,
            )
            .map_err(code)?,
        )
        .map_err(|_| "invalid_document");
    }
    if action != "plan" && !profile.allow_withdraw {
        return Err("withdrawal_disabled");
    }
    let repository = Repository::new(
        profile.git_executable.clone(),
        profile.repository_root.clone(),
        profile.source_limits,
    )
    .map_err(|_| "git_unavailable")?;
    let mut metadata = MetadataStore::open(&profile.metadata_root, &profile.metadata_minimum)
        .map_err(|_| "metadata_unavailable")?;
    if action == "reconcile" {
        return reconcile(&profile, &repository, namespace, &mut metadata, args);
    }
    let mut execution = settlement::Execution {
        enrollment: &profile.enrollment,
        repository: &repository,
        namespace: &mut namespace,
        metadata: &mut metadata,
        allow_withdraw: profile.allow_withdraw,
    };
    if action == "plan" {
        return serde_json::to_value(execution.plan().map_err(code)?)
            .map_err(|_| "invalid_document");
    }
    serde_json::to_value(
        execution
            .apply(existing.as_ref().ok_or("usage")?)
            .map_err(code)?,
    )
    .map_err(|_| "invalid_document")
}
fn reconcile(
    profile: &SettlementProfile,
    repository: &Repository,
    namespace: Namespace,
    metadata: &mut MetadataStore,
    args: &[std::ffi::OsString],
) -> Result<Value, &'static str> {
    let storage: Storage = parse(&read(Path::new(&args[3]))?)?;
    let cursor: Option<Cursor> = if args.len() == 5 {
        Some(parse(&read(Path::new(&args[4]))?)?)
    } else {
        None
    };
    let mut storage = local_placement::Open::new(
        storage,
        namespace,
        local_placement::Binding {
            root: &profile.namespace_root,
            owner: profile.enrollment.owner(),
            authority: profile.enrollment.authority(),
            namespace: profile.enrollment.namespace(),
            policy: profile.enrollment.policy(),
        },
    )?;
    storage.preflight(
        &metadata
            .finished_reader()
            .map_err(|_| "metadata_unavailable")?,
    )?;
    let mut settlement = settlement::Execution {
        enrollment: &profile.enrollment,
        repository,
        namespace: storage.namespace(),
        metadata,
        allow_withdraw: true,
    };
    let admitted = settlement.plan().map_err(code)?;
    let selected = settlement.apply(&admitted).map_err(code)?;
    if selected.observed_evidence_current != Some(true)
        || selected.conditions_current != Some(true)
        || !selected.settled
    {
        return Ok(
            json!({"schema":"tig.source-settlement-placement-response.design-1","settlement":selected,"placement":null,"storage_converged":false,"held":"stale_source"}),
        );
    }
    let placed = metadata
        .finished_reader()
        .map_err(|_| "metadata_unavailable")
        .and_then(|nodes| storage.apply(&nodes, cursor.as_ref()));
    let (current, settled, conditions) = match settlement::observe(
        &profile.enrollment,
        repository,
        storage.namespace(),
        &admitted,
    ) {
        Ok(s) => (
            s.observed_evidence_current,
            Some(s.settled),
            s.conditions_current,
        ),
        Err(_) => (None, None, None),
    };
    match placed {
        Ok((plan, placed)) => Ok(
            json!({"schema":"tig.source-settlement-placement-response.design-1","settlement":selected,"observed_evidence_current":current,"settlement_current":settled,"conditions_current":conditions,"storage_converged":if current==Some(true)&&settled==Some(true)&&conditions==Some(true){placed.storage_converged}else{Some(false)},"plan":plan,"placement":placed}),
        ),
        Err(held) => Ok(
            json!({"schema":"tig.source-settlement-placement-response.design-1","settlement":selected,"observed_evidence_current":current,"settlement_current":settled,"conditions_current":conditions,"placement":null,"storage_converged":false,"held":held}),
        ),
    }
}
