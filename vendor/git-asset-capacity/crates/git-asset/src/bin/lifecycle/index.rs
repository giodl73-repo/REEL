//! Explicit staged-index operator interface. No hook or provider initialization.
use super::profiles::Storage;
use super::*;
use git_asset::lifecycle::index;
use git_asset_store::placement::Cursor;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexProfile {
    schema: String,
    enrollment: index::Enrollment,
    git_executable: PathBuf,
    repository_root: PathBuf,
    source_limits: Limits,
    namespace_root: PathBuf,
    namespace_minimum: source_claims::Checkpoint,
    metadata_root: PathBuf,
    metadata_minimum: metadata::Checkpoint,
    #[serde(default)]
    allow_selection: bool,
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
    let profile: IndexProfile = parse(&read(Path::new(&args[2]))?)?;
    if profile.schema != "tig.git-index-profile.design-1" {
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
        Some(index::Plan::from_json(&read(Path::new(&args[3]))?).map_err(code)?)
    } else {
        None
    };
    if action == "status" {
        return serde_json::to_value(
            index::inspect(
                &profile.enrollment,
                &mut namespace,
                existing.as_ref().ok_or("usage")?,
            )
            .map_err(code)?,
        )
        .map_err(|_| "invalid_document");
    }
    if action != "plan" && !profile.allow_selection {
        return Err("selection_disabled");
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
    let mut execution = index::Execution {
        enrollment: &profile.enrollment,
        repository: &repository,
        namespace: &mut namespace,
        metadata: &mut metadata,
        allow_selection: profile.allow_selection,
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
    profile: &IndexProfile,
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
            owner: &profile.enrollment.owner,
            authority: &profile.enrollment.authority,
            namespace: &profile.enrollment.namespace,
            policy: &profile.enrollment.policy,
        },
    )?;
    storage.preflight(
        &metadata
            .finished_reader()
            .map_err(|_| "metadata_unavailable")?,
    )?;
    let mut selection = index::Execution {
        enrollment: &profile.enrollment,
        repository,
        namespace: storage.namespace(),
        metadata,
        allow_selection: true,
    };
    let admitted = selection.plan().map_err(code)?;
    let selected = selection.apply(&admitted).map_err(code)?;
    if selected.observed_index_current != Some(true) || !selected.desired_current {
        return Ok(
            json!({"schema":"tig.git-index-placement-response.design-1","selection":selected,"placement":null,"storage_converged":false,"held":"stale_index"}),
        );
    }
    let placed = metadata
        .finished_reader()
        .map_err(|_| "metadata_unavailable")
        .and_then(|nodes| storage.apply(&nodes, cursor.as_ref()));
    let current = repository
        .index()
        .ok()
        .map(|s| s.checkpoint().listing == admitted.listing);
    let selection_current = index::inspect(&profile.enrollment, storage.namespace(), &admitted)
        .ok()
        .map(|s| s.desired_current);
    match placed {
        Ok((plan, placed)) => Ok(
            json!({"schema":"tig.git-index-placement-response.design-1","selection":selected,"selection_current":selection_current,"observed_index_current":current,"storage_converged":if current==Some(true)&&selection_current==Some(true){placed.storage_converged}else{Some(false)},"plan":plan,"placement":placed}),
        ),
        Err(held) => Ok(
            json!({"schema":"tig.git-index-placement-response.design-1","selection":selected,"selection_current":selection_current,"observed_index_current":current,"placement":null,"storage_converged":false,"held":held}),
        ),
    }
}
