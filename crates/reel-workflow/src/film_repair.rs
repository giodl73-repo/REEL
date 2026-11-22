//! Exact reviewed drawing -> an unselected image task and declared consumer
//! impact. No generation, retained completion, selection or approval is implied.
use crate::{
    film_review::{self, Reader},
    *,
};
use reel_publication::film_review::ReviewMap;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema: String,
    pub reviewed_task: String,
    pub finding_key: String,
    pub task_id: String,
    pub instructions: String,
    /// Explicit additional references from the original producer task.
    pub reference_inputs: Vec<String>,
    /// A caller-authored new exact declaration for the candidate input subset.
    pub native_inputs: Option<Artifact>,
    pub output: OutputContract,
    pub input_bytes: u64,
    pub object_bytes: u64,
}
#[derive(Serialize)]
pub struct Impact {
    pub directly_affected: Vec<String>,
    pub rebuild: Vec<String>,
    pub unaffected: Vec<String>,
    pub languages: Vec<String>,
}
#[derive(Serialize)]
pub struct Prepared {
    pub schema: &'static str,
    pub baseline_plan_sha256: String,
    pub spec_sha256: String,
    pub finding: film_review::IssueDraft,
    pub old_subject: Artifact,
    pub plan: Plan,
    pub packet: Packet,
    pub impact: Impact,
    pub candidate_generated: bool,
    pub baseline_provenance_verified: bool,
    pub native_retention_verified: bool,
    pub selection_applied: bool,
    pub approvals_transferred: bool,
    pub consumers_rebuilt: bool,
}
fn path(value: &Path) -> Result<String> {
    value
        .to_str()
        .map(|s| s.replace('\\', "/"))
        .ok_or_else(|| Error {
            kind: ErrorKind::Invalid,
            code: "film-repair-producer-path".into(),
        })
}
pub fn prepare(
    baseline: &Plan,
    report: &film_review::Report,
    spec: &Spec,
    reader: &mut impl Reader,
) -> Result<Prepared> {
    let order = check_plan(baseline)?;
    let baseline_plan_sha256 = canonical_digest(baseline)?;
    let spec_sha256 = canonical_digest(spec)?;
    if spec.schema != "reel.workflow-film-image-repair.v1"
        || !token(&spec.task_id)
        || baseline.tasks.iter().any(|t| t.id == spec.task_id)
        || spec.instructions.trim().is_empty()
        || spec.instructions.len() > 16384
        || spec.reference_inputs.len() > 16
        || !spec.output.required
        || spec.input_bytes == 0
        || spec.input_bytes > 64 * 1024 * 1024 * 1024
        || spec.object_bytes == 0
        || spec.object_bytes > spec.input_bytes
    {
        return fail(ErrorKind::Invalid, "film-repair-spec");
    }
    if !matches!(
        spec.output.media_type.as_str(),
        "image/png" | "image/jpeg" | "image/webp"
    ) {
        return fail(ErrorKind::Unsupported, "film-repair-output-type");
    }
    // Resolve the current report again, rather than trusting a detached draft's
    // asserted target/hash or interpreting the finding against a latest drawing.
    let finding = film_review::draft(report, reader)?
        .into_iter()
        .find(|d| d.key == spec.finding_key)
        .ok_or_else(|| Error {
            kind: ErrorKind::Stale,
            code: "film-repair-finding-version".into(),
        })?;
    if !matches!(finding.category, film_review::Category::Picture) {
        return fail(ErrorKind::Unsupported, "film-repair-finding-category");
    }
    let reviewed = baseline
        .tasks
        .iter()
        .find(|t| t.id == spec.reviewed_task)
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-repair-reviewed-task".into(),
        })?;
    if baseline.project != finding.project
        || baseline.source_commit_for(&reviewed.id)? != finding.source_commit
        || reviewed.language.as_ref() != Some(&finding.language)
    {
        return fail(ErrorKind::Stale, "film-repair-source-snapshot");
    }
    let tool = reviewed
        .tool
        .as_ref()
        .filter(|t| t.capability == ToolCapability::FilmRender)
        .ok_or_else(|| Error {
            kind: ErrorKind::Unsupported,
            code: "film-repair-producer".into(),
        })?;
    let raw = reader.read(&report.map, 4 * 1024 * 1024)?;
    if raw.len() as u64 != report.map.bytes || digest(&raw) != report.map.sha256 {
        return fail(ErrorKind::Integrity, "film-repair-map-changed");
    }
    let map: ReviewMap = serde_json::from_slice(&raw).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "film-repair-map-json".into(),
    })?;
    if tool.publication.sha256 != map.job.sha256
        || tool.publication.bytes != map.job.bytes
        || tool.source_bindings.get(&path(&map.job.path)?) != Some(&tool.publication.logical_id)
    {
        return fail(ErrorKind::Stale, "film-repair-original-job");
    }
    let id = tool
        .source_bindings
        .get(&path(&finding.target.selected_picture.path)?)
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-repair-selected-logical-binding".into(),
        })?;
    let old_subject = reviewed
        .inputs
        .iter()
        .find(|a| &a.logical_id == id)
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-repair-selected-input".into(),
        })?;
    if old_subject.sha256 != finding.target.selected_picture.sha256
        || old_subject.bytes != finding.target.selected_picture.bytes
        || !old_subject.media_type.starts_with("image/")
    {
        return fail(ErrorKind::Stale, "film-repair-selected-picture");
    }
    let mut inputs = vec![old_subject.clone()];
    let mut ids = BTreeSet::from([old_subject.logical_id.clone()]);
    for id in &spec.reference_inputs {
        if !ids.insert(id.clone()) {
            return fail(ErrorKind::Conflict, "film-repair-reference-duplicate");
        }
        inputs.push(
            reviewed
                .inputs
                .iter()
                .find(|a| &a.logical_id == id)
                .ok_or_else(|| Error {
                    kind: ErrorKind::Denied,
                    code: "film-repair-reference-not-declared".into(),
                })?
                .clone(),
        );
    }
    let total = inputs
        .iter()
        .try_fold(0u64, |n, a| n.checked_add(a.bytes))
        .ok_or_else(|| Error {
            kind: ErrorKind::Budget,
            code: "film-repair-input-sum".into(),
        })?;
    if total > spec.input_bytes || inputs.iter().any(|a| a.bytes > spec.object_bytes) {
        return fail(ErrorKind::Budget, "film-repair-input-bytes");
    }
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: baseline.project.clone(),
        source_commit: finding.source_commit.clone(),
        task_snapshots: BTreeMap::new(),
        tasks: vec![Task {
            id: spec.task_id.clone(),
            native_inputs: spec.native_inputs.clone(),
            upstream_inputs: None,
            operation: Operation::ImageCandidate,
            tool: None,
            language: Some(finding.language.clone()),
            prompt: format!(
                "Create one unselected revised image candidate for this exact reported film target. Preserve its story/cel identity and use the declared original drawing and references. The report is an observation, not creative approval. Do not change source text, voices, clocks or other assets.\n\nAuthored repair instructions:\n{}\n\nExact finding:\n{}",
                spec.instructions, finding.body
            ),
            inputs: inputs.clone(),
            depends_on: vec![],
            outputs: BTreeMap::from([("picture".into(), spec.output.clone())]),
        }],
    };
    let packet = make_packet(&plan, &spec.task_id, BTreeMap::new())?;
    // Read only the explicitly selected original inputs; these byte checks are
    // not native record-to-leaf resolution or durable claim admission.
    for a in &inputs {
        reader.verify(a, spec.object_bytes)?;
    }
    let directly_affected: BTreeSet<_> = baseline
        .tasks
        .iter()
        .filter(|t| t.inputs.contains(old_subject))
        .map(|t| t.id.clone())
        .collect();
    let mut affected = directly_affected.clone();
    for id in &order {
        let t = baseline.tasks.iter().find(|t| &t.id == id).unwrap();
        if t.depends_on.iter().any(|d| affected.contains(d)) {
            affected.insert(id.clone());
        }
    }
    let languages = baseline
        .tasks
        .iter()
        .filter(|t| affected.contains(&t.id))
        .filter_map(|t| t.language.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let impact = Impact {
        directly_affected: order
            .iter()
            .filter(|id| directly_affected.contains(*id))
            .cloned()
            .collect(),
        rebuild: order
            .iter()
            .filter(|id| affected.contains(*id))
            .cloned()
            .collect(),
        unaffected: order
            .iter()
            .filter(|id| !affected.contains(*id))
            .cloned()
            .collect(),
        languages,
    };
    Ok(Prepared {
        schema: "reel.workflow-film-image-repair-prepared.v1",
        baseline_plan_sha256,
        spec_sha256,
        finding,
        old_subject: old_subject.clone(),
        plan,
        packet,
        impact,
        candidate_generated: false,
        baseline_provenance_verified: false,
        native_retention_verified: false,
        selection_applied: false,
        approvals_transferred: false,
        consumers_rebuilt: false,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub baseline: Plan,
    pub report: film_review::Report,
    pub repair: Spec,
    pub attempt_id: String,
    pub files: manual::FileBindings,
}
fn io(e: std::io::Error) -> Error {
    Error {
        kind: match e.kind() {
            std::io::ErrorKind::AlreadyExists => ErrorKind::Conflict,
            std::io::ErrorKind::NotFound => ErrorKind::Missing,
            std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
            std::io::ErrorKind::WouldBlock => ErrorKind::Busy,
            _ if cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)) => ErrorKind::Busy,
            _ => ErrorKind::NeedsRecovery,
        },
        code: "film-repair-package-io".into(),
    }
}
/// New local candidate package only. Original attempt capture uses the existing
/// manual result contract; native protection/retention and service calls remain
/// explicit. Existing/partial destinations are never overwritten.
pub fn export(request: &Path, pin: &str, directory: &Path) -> Result<()> {
    let mut raw = Vec::new();
    File::open(request)
        .map_err(io)?
        .take(MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(io)?;
    if raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "film-repair-request-bytes");
    }
    if digest(&raw) != pin {
        return fail(ErrorKind::Integrity, "film-repair-request-pin");
    }
    let q: Request = decode(&raw)?;
    if q.schema != "reel.workflow-film-image-repair-export.v1" || !token(&q.attempt_id) {
        return fail(ErrorKind::Invalid, "film-repair-export-schema");
    }
    if directory.try_exists().map_err(io)? {
        return fail(ErrorKind::Conflict, "film-repair-destination-exists");
    }
    let mut files = film_review::Files::new(q.files);
    let prepared = prepare(&q.baseline, &q.report, &q.repair, &mut files)?;
    let bytes = serde_json::to_vec_pretty(&prepared).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "film-repair-prepared-json".into(),
    })?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "film-repair-prepared-bytes");
    }
    fs::create_dir(directory).map_err(io)?;
    for (name, raw) in [
        ("prepared.json", bytes),
        (
            "plan.json",
            serde_json::to_vec_pretty(&prepared.plan).unwrap(),
        ),
    ] {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
            .map_err(io)?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(io)?;
    }
    manual::export_work(&prepared.packet, &q.attempt_id, &directory.join("work"))
}
