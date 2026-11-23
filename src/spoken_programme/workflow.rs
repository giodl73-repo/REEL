//! Exact runner recipe for the selected spoken edition build and check.
use super::{compiler, producer, *};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobRequest {
    schema: String,
    declaration: PathBuf,
    processing: Option<PathBuf>,
    profile: compiler::CompilationProfile,
    expected_editions: BTreeMap<String, String>,
    budget_bytes: u64,
    scratch_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RebuildRequest {
    schema: String,
    previous: PathBuf,
    declaration: PathBuf,
    processing: Option<PathBuf>,
    profile: compiler::CompilationProfile,
    budget_bytes: u64,
    scratch_bytes: u64,
}

pub fn plan_rebuild(request_path: &Path, previous_root: &Path, root: &Path) -> Result<Value> {
    let request: RebuildRequest =
        serde_json::from_slice(&producer::bounded(request_path, MAX_DECLARATION_BYTES)?)?;
    require(
        request.schema == "reel.spoken-workflow-rebuild.v1",
        "invalid spoken workflow rebuild request",
    )?;
    relative(&request.previous)?;
    relative(&request.declaration)?;
    let impact = compiler::impact_file(
        &previous_root.join(request.previous),
        &root.join(&request.declaration),
        request.profile,
    )?;
    let expected_editions: BTreeMap<_, _> = impact
        .editions
        .iter()
        .filter(|edition| matches!(edition.action, "build" | "rebuild"))
        .filter_map(|edition| {
            Some((
                edition.edition_id.clone(),
                edition.current_recipe_sha256.clone()?,
            ))
        })
        .collect();
    let job = if expected_editions.is_empty() {
        None
    } else {
        Some(build_job(
            JobRequest {
                schema: "reel.spoken-workflow-job.v1".into(),
                declaration: request.declaration,
                processing: request.processing,
                profile: request.profile,
                expected_editions,
                budget_bytes: request.budget_bytes,
                scratch_bytes: request.scratch_bytes,
            },
            root,
            Some(&impact.current_declaration_sha256),
        )?)
    };
    let job_sha256 = job
        .as_ref()
        .map(|job| serde_json::to_vec(job).map(|bytes| compiler::digest(&bytes)))
        .transpose()?;
    Ok(
        json!({"schema":"reel.spoken-workflow-rebuild-plan.v1","impact":impact,"job":job,
        "job_sha256":job_sha256}),
    )
}

/// Execute the exact approved recipe through the existing local registry runner.
/// Keep the recipe on failures so an uncertain attempt is never silently repeated.
pub fn dispatch_rebuild(
    project: &Path,
    request: &Path,
    previous_root: &Path,
    root: &Path,
    expected_job: &str,
    dispatch: &Path,
) -> Result<Value> {
    let plan = plan_rebuild(request, previous_root, root)?;
    require(!plan["job"].is_null(), "spoken rebuild plan has no work")?;
    let bytes = serde_json::to_vec(&plan["job"])?;
    require(
        compiler::digest(&bytes) == expected_job,
        "spoken rebuild job pin changed",
    )?;
    fs::create_dir(dispatch).context("spoken dispatch directory must be new")?;
    let recipe = dispatch.join("recipe.json");
    let mut request = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&recipe)?;
    request.write_all(&bytes)?;
    request.sync_all()?;
    drop(request);
    let result = reel_assets::jobs::run(project, &recipe).map_err(|error| {
        anyhow::anyhow!("spoken managed dispatch requires reconciliation: {error}")
    })?;
    let raw = serde_json::to_vec(&result)?;
    let mut staged = tempfile::NamedTempFile::new_in(dispatch)?;
    staged.write_all(&raw)?;
    staged.as_file().sync_all()?;
    staged.persist_noclobber(dispatch.join("result.json"))?;
    Ok(
        json!({"schema":"reel.spoken-workflow-dispatch.v1","job_sha256":expected_job,
        "result_sha256":compiler::digest(&raw)}),
    )
}

fn relative(path: &Path) -> Result<String> {
    require(
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_))),
        "spoken workflow control must be relative",
    )?;
    Ok(path
        .to_str()
        .context("spoken workflow path is not UTF-8")?
        .replace('\\', "/"))
}

fn add_input(inputs: &mut BTreeMap<String, Value>, item: &FileRef) -> Result<()> {
    let name = relative(&item.path)?;
    let pin = json!({"sha256":item.sha256,"bytes":item.bytes});
    if let Some(existing) = inputs.get(&name) {
        require(existing == &pin, "conflicting spoken workflow input")?;
    } else {
        inputs.insert(name, pin);
    }
    Ok(())
}

fn read_control(
    root: &Path,
    path: &Path,
    limit: usize,
    inputs: &mut BTreeMap<String, Value>,
) -> Result<Vec<u8>> {
    let _ = relative(path)?;
    let bytes = producer::bounded(&root.join(path), limit)?;
    let item = FileRef {
        path: path.into(),
        sha256: compiler::digest(&bytes),
        bytes: bytes.len() as u64,
    };
    crate::scene_delivery::checked_file(root, &item)?;
    add_input(inputs, &item)?;
    Ok(bytes)
}

pub fn describe_job(request_path: &Path, root: &Path) -> Result<Value> {
    let request: JobRequest =
        serde_json::from_slice(&producer::bounded(request_path, MAX_DECLARATION_BYTES)?)?;
    require(
        request.schema == "reel.spoken-workflow-job.v1" && !request.expected_editions.is_empty(),
        "invalid spoken workflow job request",
    )?;
    build_job(request, root, None)
}

fn build_job(
    request: JobRequest,
    root: &Path,
    expected_declaration: Option<&str>,
) -> Result<Value> {
    let declaration_path = relative(&request.declaration)?;
    let asset_root_path = request
        .declaration
        .parent()
        .context("spoken declaration parent missing")?;
    let asset_root = root.join(asset_root_path);
    let mut inputs = BTreeMap::new();
    let declaration = read_control(
        root,
        &request.declaration,
        MAX_DECLARATION_BYTES,
        &mut inputs,
    )?;
    let document = parse(&declaration)?;
    let runtime = producer::current_runtime()?;
    let full = compiler::compile_with_inputs(&document, &asset_root, &runtime, request.profile)?;
    require(
        expected_declaration.is_none_or(|hash| hash == full.declaration_semantics_sha256),
        "spoken declaration changed while planning rebuild",
    )?;
    let edition_ids: Vec<_> = request.expected_editions.keys().cloned().collect();
    let selected = compiler::select_editions(full, &edition_ids)?;
    let mut outputs = BTreeMap::from([
        ("spoken-plan.json".to_owned(), 16 * 1024 * 1024),
        ("spoken-candidate/receipt.json".to_owned(), 16 * 1024 * 1024),
        ("spoken-access/access.json".to_owned(), 16 * 1024 * 1024),
    ]);
    for (index, edition) in selected.editions.iter().enumerate() {
        require(
            request.expected_editions[&edition.recipe.edition_id] == edition.recipe_sha256,
            "spoken workflow edition recipe changed",
        )?;
        outputs.insert(
            format!("spoken-candidate/edition-{index:03}.wav"),
            edition.duration_samples * 6 + 44,
        );
        outputs.insert(
            format!("spoken-access/edition-{index:03}.txt"),
            4 * 1024 * 1024,
        );
        for cue in &edition.recipe.cues {
            let mut source = cue.performance.source.clone();
            source.path = asset_root_path.join(source.path);
            add_input(&mut inputs, &source)?;
        }
    }
    let selected_ids = compiler::selected_ids(&document);
    for take in &document.performances {
        if selected_ids.contains(&take.id) {
            let mut alignment = take.alignment.clone();
            alignment.path = asset_root_path.join(alignment.path);
            add_input(&mut inputs, &alignment)?;
        }
    }
    let profile = match request.profile {
        compiler::CompilationProfile::Engineering => "engineering",
        compiler::CompilationProfile::Spoken => "spoken",
    };
    let assets = if asset_root_path.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        relative(asset_root_path)?
    };
    let mut compile = vec![
        "spoken-edition-compile",
        &declaration_path,
        "--output",
        "spoken-plan.json",
        "--profile",
        profile,
    ];
    for id in &edition_ids {
        compile.extend(["--select-edition", id]);
    }
    let mut produce = vec![
        "spoken-edition-produce",
        "spoken-plan.json",
        "--declaration",
        &declaration_path,
        "--asset-root",
        &assets,
        "--output",
        "spoken-candidate",
    ];
    let mut check = vec![
        "spoken-edition-check",
        &declaration_path,
        "spoken-plan.json",
        "--asset-root",
        &assets,
        "--candidate",
        "spoken-candidate",
        "--write-access",
        "spoken-access",
    ];
    let processing_path = if let Some(path) = &request.processing {
        let bytes = read_control(root, path, MAX_DECLARATION_BYTES, &mut inputs)?;
        producer::validate_processing(&producer::parse_processing(&bytes)?, &selected)?;
        Some(relative(path)?)
    } else {
        None
    };
    if let Some(path) = &processing_path {
        produce.extend(["--processing", path]);
        check.extend(["--processing", path]);
    }
    let outputs: BTreeMap<_, _> = outputs
        .into_iter()
        .map(|(path, max_bytes)| (path.clone(), json!({"path":path,"max_bytes":max_bytes})))
        .collect();
    let timeout = request.profile.timeout_seconds();
    let steps: Vec<_> = [compile, produce, check]
        .into_iter()
        .map(|args| json!({"tool":"spoken-worker","args":args,"timeout_seconds":timeout}))
        .collect();
    let job = json!({
        "budget_bytes":request.budget_bytes,"scratch_bytes":request.scratch_bytes,
        "inputs":inputs,"tools":{},"installed_tools":{
            "spoken-worker":{"path":std::env::current_exe()?.canonicalize()?,"sha256":runtime.sha256,"bytes":runtime.bytes}},
        "prepare_output_parents":false,"steps":steps,"outputs":outputs
    });
    reel_assets::jobs::check_request(&serde_json::to_vec(&job)?)
        .map_err(|error| anyhow::anyhow!("spoken workflow recipe refused: {error}"))?;
    Ok(job)
}
