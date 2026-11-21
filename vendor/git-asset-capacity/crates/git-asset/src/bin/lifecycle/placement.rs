//! Private operator composition. Event authentication remains external.
use super::profiles::Storage;
use super::*;
use git_asset_store::{
    existing::{Profile as SourceProfile, ReadCache},
    ledger::CapacityLedger,
    local::LocalStore,
    placement::{self, Cursor, Local},
};
pub(super) fn reconcile(args: &[std::ffi::OsString]) -> Result<Value, &'static str> {
    if !(4..=5).contains(&args.len()) {
        return Err("usage");
    }
    let profile: Profile = parse(&read(Path::new(&args[1]))?)?;
    let storage: Storage = parse(&read(Path::new(&args[3]))?)?;
    if profile.schema != "tig.git-lifecycle-profile.design-1"
        || storage.schema != "tig.placement-profile.design-1"
    {
        return Err("invalid_document");
    }
    if !profile.allow_selection {
        return Err("selection_disabled");
    }
    profile.enrollment.id().map_err(code)?;
    storage.config.id().map_err(placement_code)?;
    if storage.providers.len() != storage.config.providers.len()
        || storage.namespaces.len() != storage.config.namespaces.len()
        || storage.config.owner != profile.enrollment.owner
        || storage.config.authority != profile.enrollment.authority
        || !storage.config.namespaces.iter().any(|n| {
            n.namespace == profile.enrollment.namespace && n.policy == profile.enrollment.policy
        })
    {
        return Err("authority");
    }
    for path in [
        &profile.git_executable,
        &profile.repository_root,
        &profile.namespace_root,
        &profile.metadata_root,
    ] {
        absolute(path)?;
    }
    for n in &storage.namespaces {
        absolute(&n.root)?;
    }
    for p in &storage.providers {
        absolute(&p.root)?;
        absolute(&p.ledger_root)?;
    }
    let source_digest = Digest::blob(
        &identity::canonical_json(&storage.byte_source).map_err(|_| "invalid_document")?,
    );
    if source_digest != storage.config.seed {
        return Err("authority");
    }
    let seed_profile: SourceProfile =
        serde_json::from_value(storage.byte_source).map_err(|_| "invalid_document")?;
    let mut seed = ReadCache::new(seed_profile).map_err(|_| "source_unavailable")?;
    let request: Request = parse(&read(Path::new(&args[2]))?)?;
    if request.schema != "tig.git-lifecycle-request.design-1"
        || request.repository != profile.enrollment.repository
        || request.ref_name != profile.enrollment.ref_name
    {
        return Err("authority");
    }
    let commit = request
        .commit
        .as_deref()
        .map(GitId::parse)
        .transpose()
        .map_err(|_| "invalid_document")?;
    let cursor: Option<Cursor> = if args.len() == 5 {
        Some(parse(&read(Path::new(&args[4]))?)?)
    } else {
        None
    };
    let repository = Repository::new(
        profile.git_executable,
        profile.repository_root,
        profile.source_limits,
    )
    .map_err(|_| "git_unavailable")?;
    let mut namespace = Namespace::open(&profile.namespace_root, &profile.namespace_minimum)
        .map_err(|_| "journal_unavailable")?;
    let mut metadata = MetadataStore::open(&profile.metadata_root, &profile.metadata_minimum)
        .map_err(|_| "metadata_unavailable")?;
    let mut namespaces = storage
        .namespaces
        .iter()
        .map(|n| Namespace::open(&n.root, &n.minimum).map_err(|_| "journal_unavailable"))
        .collect::<Result<Vec<_>, _>>()?;
    let source_index = namespaces
        .iter()
        .position(|n| n.identity() == &profile.enrollment.namespace)
        .ok_or("authority")?;
    if fs::canonicalize(&profile.namespace_root).map_err(|_| "journal_unavailable")?
        != fs::canonicalize(&storage.namespaces[source_index].root)
            .map_err(|_| "journal_unavailable")?
    {
        return Err("authority");
    }
    let stores = storage
        .providers
        .iter()
        .map(|p| LocalStore::open(&p.root).map_err(|_| "provider_unavailable"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ledgers = storage
        .providers
        .iter()
        .map(|p| {
            CapacityLedger::open(&p.ledger_root, &p.minimum).map_err(|_| "capacity_unavailable")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut places = stores
        .iter()
        .zip(ledgers.iter_mut())
        .zip(storage.config.providers.iter())
        .map(|((store, ledger), p)| Local {
            store,
            ledger,
            job: &p.job,
        })
        .collect::<Vec<_>>();
    // Validate the complete private placement binding before changing selection.
    let nodes = metadata
        .finished_reader()
        .map_err(|_| "metadata_unavailable")?;
    placement::Execution {
        config: &storage.config,
        namespaces: &mut namespaces,
        nodes: &nodes,
        places: &mut places,
        seed: &mut seed,
        seed_identity: &source_digest,
        allow_copy: false,
        allow_eviction: false,
        allow_reacquisition: false,
        allow_cancellation: false,
    }
    .plan(None)
    .map_err(placement_code)?;
    let mut selection = Execution {
        enrollment: &profile.enrollment,
        repository: &repository,
        namespace: &mut namespace,
        metadata: &mut metadata,
        allow_selection: true,
    };
    let selection_plan = selection.plan(commit.as_ref()).map_err(code)?;
    let admitted_commit = GitId::parse(&selection_plan.commit).map_err(|_| "invalid_document")?;
    let selected = selection.apply(&selection_plan).map_err(code)?;
    if selected.observed_ref_current != Some(true) || !selected.desired_current {
        return Ok(
            json!({"schema":"tig.git-placement-response.design-1","selection":selected,"placement":null,"storage_converged":false,"held":"stale_ref"}),
        );
    }
    let nodes = metadata
        .finished_reader()
        .map_err(|_| "metadata_unavailable")?;
    let mut execution = placement::Execution {
        config: &storage.config,
        namespaces: &mut namespaces,
        nodes: &nodes,
        places: &mut places,
        seed: &mut seed,
        seed_identity: &source_digest,
        allow_copy: storage.allow_copy,
        allow_eviction: storage.allow_eviction,
        allow_reacquisition: storage.allow_reacquisition,
        allow_cancellation: storage.allow_cancellation,
    };
    let plan = execution.plan(cursor.as_ref()).map_err(placement_code)?;
    let placed = execution.apply(&plan).map_err(placement_code)?;
    let current = repository
        .named_commit(&profile.enrollment.ref_name)
        .map_err(|_| "git_unavailable")?
        == admitted_commit;
    let selection_current =
        lifecycle::inspect_selection(&profile.enrollment, &mut namespace, &selection_plan)
            .ok()
            .map(|s| s.desired_current);
    // A Git change requests catch-up; a provider report remains independently
    // scoped to its desired basis and page even when the ref has since moved.
    Ok(
        json!({"schema":"tig.git-placement-response.design-1","selection":selected,"selection_current":selection_current,"observed_ref_current":current,"storage_converged":if current&&selection_current==Some(true) {placed.storage_converged} else {Some(false)},"plan":plan,"placement":placed}),
    )
}
pub(super) fn placement_code(e: placement::Error) -> &'static str {
    use placement::Error::*;
    match e {
        Invalid => "invalid_document",
        WrongEnrollment => "authority",
        Stale => "stale",
        Budget => "budget",
        Disabled => "placement_disabled",
        Busy => "busy",
        Unavailable => "provider_unavailable",
        Integrity => "integrity",
        Recovery => "needs_recovery",
        Protected => "protected",
        Retained => "retained",
        MissingSource => "source_unavailable",
        ReacquisitionRequired => "reacquisition_required",
        FinalRetirement => "final_retirement",
    }
}
