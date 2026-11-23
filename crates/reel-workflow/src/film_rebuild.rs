//! Exact affected-film rebuild planning. No provider, protection, selection,
//! approval transfer or media production follows from this metadata contract.
use crate::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parent {
    pub packet: Packet,
    pub result: ReturnedResult,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Consumer {
    pub task_id: String,
    pub control_task: String,
    pub control_output: String,
    pub replacement_path: String,
    /// A new alias retains the original job as an explicit baseline input.
    pub baseline_job_path: String,
    /// Host-created exact native declaration for this new input closure. None
    /// is an explicitly unfinished native binding, never reuse of the old pin.
    pub native_inputs: Option<Artifact>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub baseline: Plan,
    pub old_subject: Artifact,
    pub image: Parent,
    pub image_output: String,
    pub controls: BTreeMap<String, Parent>,
    pub consumers: Vec<Consumer>,
}
#[derive(Serialize)]
pub struct Prepared {
    pub schema: &'static str,
    pub baseline_sha256: String,
    pub plan: Plan,
    pub bindings: BTreeMap<String, upstream::Bindings>,
    pub render_tasks: Vec<String>,
    pub affected_languages: Vec<String>,
    pub unaffected: Vec<String>,
    /// These consumers are withheld from the executable plan until new exact
    /// parent outputs can replace their former input pins.
    pub pending_consumers: Vec<String>,
    pub native_binding_required: Vec<String>,
    pub selection_verified: bool,
    pub approvals_transferred: bool,
    pub consumers_rebuilt: bool,
}
fn parent(project: &str, p: &Parent) -> Result<()> {
    check_result(&p.packet, &p.result)?;
    if p.packet.project != project {
        return fail(ErrorKind::Conflict, "film-rebuild-parent-project");
    }
    Ok(())
}
pub fn prepare(q: &Request) -> Result<Prepared> {
    let order = check_plan(&q.baseline)?;
    if q.schema != "reel.workflow-film-rebuild.v1"
        || q.consumers.is_empty()
        || q.consumers.len() > MAX_TASKS
    {
        return fail(ErrorKind::Invalid, "film-rebuild-request");
    }
    parent(&q.baseline.project, &q.image)?;
    let image_result_sha256 = canonical_digest(&q.image.result)?;
    if q.image.packet.task.operation != Operation::ImageCandidate
        || !q.image.packet.task.inputs.contains(&q.old_subject)
    {
        return fail(ErrorKind::Conflict, "film-rebuild-original-image");
    }
    let image = q
        .image
        .result
        .artifacts
        .get(&q.image_output)
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-rebuild-image-output".into(),
        })?;
    if !image.media_type.starts_with("image/")
        || (image.sha256.as_str(), image.bytes)
            == (q.old_subject.sha256.as_str(), q.old_subject.bytes)
    {
        return fail(ErrorKind::Invalid, "film-rebuild-new-image");
    }
    let direct: BTreeSet<_> = q
        .baseline
        .tasks
        .iter()
        .filter(|t| t.inputs.contains(&q.old_subject))
        .map(|t| t.id.clone())
        .collect();
    if direct.is_empty()
        || direct != q.consumers.iter().map(|c| c.task_id.clone()).collect()
        || q.consumers
            .iter()
            .map(|c| &c.task_id)
            .collect::<BTreeSet<_>>()
            .len()
            != q.consumers.len()
    {
        return fail(ErrorKind::Conflict, "film-rebuild-complete-consumer-set");
    }
    if q.controls.keys().cloned().collect::<BTreeSet<_>>()
        != q.consumers.iter().map(|c| c.control_task.clone()).collect()
    {
        return fail(ErrorKind::Conflict, "film-rebuild-control-set");
    }
    let mut affected = direct.clone();
    for id in &order {
        let t = q.baseline.tasks.iter().find(|t| &t.id == id).unwrap();
        if t.depends_on.iter().any(|d| affected.contains(d)) {
            affected.insert(id.clone());
        }
    }
    let pending: BTreeSet<_> = affected.difference(&direct).cloned().collect();
    // A direct renderer can also be downstream of another changed consumer.
    // Its old dependency receipt cannot authorize the new upstream render.
    // Withhold that topology until exact result rebinding is supported.
    if q.baseline
        .tasks
        .iter()
        .any(|t| direct.contains(&t.id) && t.depends_on.iter().any(|id| affected.contains(id)))
    {
        return fail(ErrorKind::Unsupported, "film-rebuild-affected-prerequisite");
    }
    let mut tasks: BTreeMap<_, _> = q
        .baseline
        .tasks
        .iter()
        .filter(|t| !pending.contains(&t.id))
        .map(|t| (t.id.clone(), t.clone()))
        .collect();
    let mut snapshots: BTreeMap<_, _> = tasks
        .keys()
        .map(|id| Ok((id.clone(), q.baseline.source_commit_for(id)?.into())))
        .collect::<Result<_>>()?;
    for p in std::iter::once(&q.image).chain(q.controls.values()) {
        parent(&q.baseline.project, p)?;
        if tasks
            .insert(p.packet.task.id.clone(), p.packet.task.clone())
            .is_some()
            || q.baseline.tasks.iter().any(|t| t.id == p.packet.task.id)
        {
            return fail(ErrorKind::Conflict, "film-rebuild-parent-task-collision");
        }
        snapshots.insert(p.packet.task.id.clone(), p.packet.source_commit.clone());
    }
    let mut bindings = BTreeMap::new();
    let mut required = Vec::new();
    let mut languages = BTreeSet::new();
    for change in &q.consumers {
        let t = tasks.get_mut(&change.task_id).unwrap();
        // Existing upstream closures require their original binding documents
        // to be merged, not replacement with just this repair's two parents.
        if t.upstream_inputs.is_some() {
            return fail(
                ErrorKind::Unsupported,
                "film-rebuild-existing-upstream-closure",
            );
        }
        let tool = t
            .tool
            .as_mut()
            .filter(|t| t.capability == ToolCapability::FilmRender && t.part.is_none())
            .ok_or_else(|| Error {
                kind: ErrorKind::Unsupported,
                code: "film-rebuild-direct-producer".into(),
            })?;
        let control = &q.controls[&change.control_task];
        let control_kind = match (&control.packet.task.operation, &control.packet.task.tool) {
            (Operation::TextProposal, None) => true,
            (Operation::DeterministicTool, Some(tool)) => {
                tool.capability == ToolCapability::FilmSceneControl
            }
            _ => false,
        };
        if control.packet.task.id != change.control_task
            || !control_kind
            || control.packet.task.language != t.language
            || !control.packet.task.inputs.contains(&tool.publication)
            || !control.packet.task.inputs.contains(image)
            || control
                .packet
                .dependencies
                .get(&q.image.packet.task.id)
                .is_none_or(|d| {
                    d.result_sha256 != image_result_sha256
                        || d.artifacts != q.image.result.artifacts
                })
        {
            return fail(ErrorKind::Conflict, "film-rebuild-control-subjects");
        }
        let job = control
            .result
            .artifacts
            .get(&change.control_output)
            .filter(|a| a.media_type == "application/json")
            .ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "film-rebuild-control-output".into(),
            })?;
        if !portable_source(&change.replacement_path)
            || !portable_source(&change.baseline_job_path)
            || tool.source_bindings.contains_key(&change.replacement_path)
            || tool.source_bindings.contains_key(&change.baseline_job_path)
            || change.replacement_path == change.baseline_job_path
            || [image, job]
                .iter()
                .any(|a| t.inputs.iter().any(|old| old.logical_id == a.logical_id))
            || image.logical_id == job.logical_id
        {
            return fail(ErrorKind::Conflict, "film-rebuild-input-alias");
        }
        let old = tool.publication.clone();
        let mut found = false;
        for id in tool.source_bindings.values_mut() {
            if *id == old.logical_id {
                *id = job.logical_id.clone();
                found = true;
            }
        }
        if !found {
            return fail(ErrorKind::Missing, "film-rebuild-original-job-alias");
        }
        tool.source_bindings
            .insert(change.baseline_job_path.clone(), old.logical_id);
        tool.source_bindings
            .insert(change.replacement_path.clone(), image.logical_id.clone());
        tool.publication = job.clone();
        t.inputs.extend([image.clone(), job.clone()]);
        for id in [&q.image.packet.task.id, &control.packet.task.id] {
            if !t.depends_on.contains(id) {
                t.depends_on.push(id.clone());
            }
        }
        let b = upstream::Bindings {
            schema: "reel.workflow-upstream-inputs.v2".into(),
            streaming: None,
            inputs: BTreeMap::from([
                (
                    image.logical_id.clone(),
                    upstream::Reference {
                        task_id: q.image.packet.task.id.clone(),
                        artifact: upstream::Selector::Output {
                            name: q.image_output.clone(),
                        },
                    },
                ),
                (
                    job.logical_id.clone(),
                    upstream::Reference {
                        task_id: control.packet.task.id.clone(),
                        artifact: upstream::Selector::Output {
                            name: change.control_output.clone(),
                        },
                    },
                ),
            ]),
        };
        let raw = b.canonical_bytes()?;
        t.upstream_inputs = Some(Artifact {
            logical_id: format!("workflow/rebuild/{}/origins", t.id),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        });
        t.native_inputs = change.native_inputs.clone();
        if t.native_inputs.is_none() {
            required.push(t.id.clone());
        }
        if let Some(lang) = &t.language {
            languages.insert(lang.clone());
        }
        bindings.insert(t.id.clone(), b);
    }
    let plan = Plan {
        schema: SCOPED_PLAN_SCHEMA.into(),
        project: q.baseline.project.clone(),
        source_commit: q.baseline.source_commit.clone(),
        task_snapshots: snapshots,
        tasks: tasks.into_values().collect(),
    };
    check_plan(&plan)?;
    Ok(Prepared {
        schema: "reel.workflow-film-rebuild-plan.v1",
        baseline_sha256: canonical_digest(&q.baseline)?,
        plan,
        bindings,
        render_tasks: order
            .iter()
            .filter(|id| direct.contains(*id))
            .cloned()
            .collect(),
        affected_languages: languages.into_iter().collect(),
        unaffected: order
            .iter()
            .filter(|id| !affected.contains(*id))
            .cloned()
            .collect(),
        pending_consumers: order
            .iter()
            .filter(|id| pending.contains(*id))
            .cloned()
            .collect(),
        native_binding_required: required,
        selection_verified: false,
        approvals_transferred: false,
        consumers_rebuilt: false,
    })
}

/// Export immutable preparation files only. Native admission and actual
/// protected dispatch use their explicit adapters after binding is complete.
pub fn export(request: &std::path::Path, pin: &str, directory: &std::path::Path) -> Result<()> {
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Write},
    };
    let io = |error: std::io::Error| Error {
        kind: match error.kind() {
            std::io::ErrorKind::AlreadyExists => ErrorKind::Conflict,
            std::io::ErrorKind::NotFound => ErrorKind::Missing,
            _ => ErrorKind::NeedsRecovery,
        },
        code: "film-rebuild-package-io".into(),
    };
    let mut raw = Vec::new();
    File::open(request)
        .map_err(io)?
        .take(MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(io)?;
    if raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "film-rebuild-request-bytes");
    }
    if digest(&raw) != pin {
        return fail(ErrorKind::Integrity, "film-rebuild-request-pin");
    }
    let q: Request = decode(&raw)?;
    let prepared = prepare(&q)?;
    let mut files = BTreeMap::from([
        (
            "prepared.json".to_owned(),
            serde_json::to_vec_pretty(&prepared).unwrap(),
        ),
        (
            "plan.json".to_owned(),
            serde_json::to_vec_pretty(&prepared.plan).unwrap(),
        ),
    ]);
    for (id, binding) in &prepared.bindings {
        files.insert(format!("{id}-origins.json"), binding.canonical_bytes()?);
    }
    if files.values().any(|raw| raw.len() > MAX_CONTROL_BYTES) {
        return fail(ErrorKind::Budget, "film-rebuild-package-bytes");
    }
    fs::create_dir(directory).map_err(io)?;
    for (name, raw) in files {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
            .map_err(io)?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(io)?;
    }
    Ok(())
}
