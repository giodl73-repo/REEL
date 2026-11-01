//! Trusted worker: source-owned comparisons and native assembly inside containment.
use super::*;
use crate::{tig_presentation, tig_scene_production::jobs::reference::ReferencePlan};

fn check(t: &Ticket, journal: &mut SourceJournal) -> Result<()> {
    let state = journal
        .source(&key(&t.source_id)?)?
        .context("episode root source missing")?;
    if state.owner != key(&t.owner)?
        || state.revision != t.root_revision
        || state.pending.is_some()
        || state
            .current
            .as_ref()
            .is_none_or(|current| current.selection != t.resources)
    {
        bail!("episode root source changed");
    }
    Ok(())
}
fn identity(t: &Ticket, phase_name: &str) -> Result<Identity> {
    let source = phase(&t.source_id, &t.operation, phase_name)?;
    if source == key(&t.source_id)? {
        bail!("episode child cannot alias runner source");
    }
    Ok(Identity {
        source,
        owner: key(&t.owner)?,
    })
}
fn write_report(path: &Path, value: &Value) -> Result<()> {
    let raw = identity::canonical_json(value)?;
    if raw.len() > 65536 {
        bail!("episode worker report exceeds budget");
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&raw)?;
    file.sync_all()?;
    Ok(())
}
fn temporary_sources(t: &Ticket) -> Result<Vec<String>> {
    t.inputs
        .scenes
        .iter()
        .map(|s| format!("scene/{}", s.id))
        .chain(
            t.inputs
                .presentations
                .iter()
                .map(|s| format!("presentation/{}", s.id)),
        )
        .chain(std::iter::once("parent".to_owned()))
        .map(|label| identity(t, &label).map(|id| id.source.as_str().to_owned()))
        .collect()
}
/// Internal trusted route. Use run() for capacity, process-tree containment,
/// timeout, result retention and root-source authority.
pub fn worker(path: &Path, expected: &Object) -> Result<Value> {
    let t: Ticket = read(path, 65536, Some(expected))?;
    if t.schema != TICKET {
        bail!("unsupported episode worker");
    }
    let wait = MetadataWait::default();
    let programs = BoundPrograms::bind(&t.inputs.runtime)?;
    let (mut source, context) = restore(&t.inputs, &t.metadata_root, &t.metadata_minimum, &wait)?;
    let templates = records::prepare(
        &t.inputs,
        &t.metadata_root,
        &t.metadata_minimum,
        &context,
        &t.output_roots,
    )?;
    let mut journal = SourceJournal::open(&t.journal_root, &t.journal_minimum)?;
    check(&t, &mut journal)?;
    let coordination = Coordination {
        root: t.coordination_root.clone(),
    };
    let budget = protection(t.max_objects, t.max_logical_bytes)?;
    let preparation = Path::new("episode-prepare");
    fs::create_dir(preparation)?;
    let mut store = Waiting::new(
        wait.run(|| MetadataStore::open(&t.metadata_root, &t.metadata_minimum))?,
        &wait,
    );
    let root = planning::freeze(
        &t.inputs,
        &mut source,
        &context,
        &programs,
        &mut store,
        preparation,
    )?;
    if root.recipe.fingerprint() != &t.recipe || root.resources.selection() != &t.resources {
        bail!("episode worker input closure differs from admission");
    }
    admit(&root.resources, &budget)?;
    let mut scene_sources = t
        .inputs
        .scenes
        .iter()
        .map(|_| {
            Source::from_archive(
                serde_json::from_value(t.inputs.source.clone())?,
                &mut store,
                TreeLimits::default(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let scene_contexts = t
        .inputs
        .scenes
        .iter()
        .zip(&mut scene_sources)
        .enumerate()
        .map(|(i, (spec, source))| {
            if spec.compiled {
                tig_scene::freeze_render_context(
                    source,
                    &spec.request,
                    &preparation.join(format!("owned-context-{i}")),
                    &mut store,
                    TreeLimits::default(),
                )
            } else {
                tig_scene::freeze_context(source, &spec.request, &mut store, TreeLimits::default())
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let native = t
        .inputs
        .scenes
        .iter()
        .zip(&scene_contexts)
        .zip(&mut scene_sources)
        .enumerate()
        .map(|(i, ((spec, context), source))| {
            if spec.compiled {
                jobs::JobPlan::from_render_context(
                    source,
                    &spec.request,
                    context,
                    &programs,
                    &preparation.join(format!("owned-recipe-{i}")),
                )
            } else {
                jobs::JobPlan::from_context(source, &spec.request, context, &programs)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let references = t
        .inputs
        .scenes
        .iter()
        .zip(&native)
        .zip(&mut scene_sources)
        .enumerate()
        .map(|(i, ((spec, native), source))| {
            ReferencePlan::compile(
                source,
                native,
                &spec.output_id,
                &spec.output_definition,
                &preparation.join(format!("owned-scene-{i}")),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let scene_resources = references
        .iter()
        .map(|reference| reference.freeze(&mut store, TreeLimits::default()))
        .collect::<Result<Vec<_>>>()?;
    let mut presentation_sources = t
        .inputs
        .presentations
        .iter()
        .map(|_| {
            Source::from_archive(
                serde_json::from_value(t.inputs.source.clone())?,
                &mut store,
                TreeLimits::default(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let presentation_contexts = t
        .inputs
        .presentations
        .iter()
        .zip(&mut presentation_sources)
        .map(|(spec, source)| {
            tig_presentation::freeze_context(
                source,
                &spec.request,
                &mut store,
                TreeLimits::default(),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let presentation_plans = t
        .inputs
        .presentations
        .iter()
        .zip(&presentation_contexts)
        .zip(&mut presentation_sources)
        .map(|((spec, context), source)| {
            tig_presentation::jobs::JobPlan::from_context(
                source,
                &spec.request,
                context,
                &programs,
                &spec.output_id,
                &spec.output_definition,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let presentation_resources = presentation_plans
        .iter()
        .map(|plan| plan.freeze(&mut store, TreeLimits::default()))
        .collect::<Result<Vec<_>>>()?;
    let checkpoint = wait.run(|| store.inner.finish())?;
    drop(store);
    let mut reader = Waiting::new(
        wait.run(|| MetadataReader::open(&t.metadata_root, &checkpoint))?,
        &wait,
    );
    let scene_owners = references
        .iter()
        .zip(&scene_resources)
        .zip(&t.inputs.scenes)
        .map(|((reference, resources), spec)| {
            reference
                .readback(resources.selection(), &mut reader, TreeLimits::default())?
                .protect(
                    &source,
                    &mut journal,
                    &coordination,
                    identity(&t, &format!("scene/{}", spec.id))?,
                    0,
                    &phase(
                        &t.source_id,
                        &t.operation,
                        &format!("protect/scene/{}", spec.id),
                    )?,
                    &budget,
                )
        })
        .collect::<Result<Vec<_>>>()?;
    let presentation_owners = presentation_plans
        .iter()
        .zip(&presentation_resources)
        .zip(&t.inputs.presentations)
        .map(|((plan, resources), spec)| {
            plan.readback(resources.selection(), &mut reader, TreeLimits::default())?
                .protect(
                    &source,
                    &mut journal,
                    &coordination,
                    identity(&t, &format!("presentation/{}", spec.id))?,
                    0,
                    &phase(
                        &t.source_id,
                        &t.operation,
                        &format!("protect/presentation/{}", spec.id),
                    )?,
                    &budget,
                )
        })
        .collect::<Result<Vec<_>>>()?;
    drop(reader);
    let reference_budget = crate::scene_primary_reference::Budget {
        max_staged_bytes: t.reference_staging_bytes,
        timeout_seconds: t.timeout_seconds.min(3600),
        max_picture_processes: t.reference_picture_processes,
    };
    let stage = Path::new("reference-stage");
    fs::create_dir(stage)?;
    let stage = stage.canonicalize()?;
    let scenes = scene_owners
        .iter()
        .zip(&mut scene_sources)
        .zip(&t.inputs.scenes)
        .map(|((owner, source), spec)| {
            check(&t, &mut journal)?;
            Ok((
                spec.id.clone(),
                owner
                    .verify_selected_master(source, &mut journal, &stage, &reference_budget)
                    .with_context(|| {
                        format!(
                            "episode scene {} primary-reference verification failed",
                            spec.id
                        )
                    })?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let presentations = presentation_owners
        .iter()
        .zip(&mut presentation_sources)
        .zip(&t.inputs.presentations)
        .map(|((owner, source), spec)| {
            check(&t, &mut journal)?;
            Ok((
                spec.id.clone(),
                owner
                    .verify_selected_master(source, &mut journal, &stage, t.reference_staging_bytes)
                    .with_context(|| {
                        format!(
                            "episode presentation {} primary-reference verification failed",
                            spec.id
                        )
                    })?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let plan = tig_episode::CompositionJobPlan::from_verified_components(
        &mut source,
        &t.inputs.request,
        &context,
        &programs,
        &scenes,
        &presentations,
    )?;
    let mut store = Waiting::new(
        wait.run(|| MetadataStore::open(&t.metadata_root, &checkpoint))?,
        &wait,
    );
    let frozen = plan.freeze(&mut store, TreeLimits::default())?;
    admit(&frozen, &budget)?;
    let checkpoint = wait.run(|| store.inner.finish())?;
    drop(store);
    let mut reader = Waiting::new(
        wait.run(|| MetadataReader::open(&t.metadata_root, &checkpoint))?,
        &wait,
    );
    let children = tig_episode::ProtectedScenes::without_legacy_jobs(&source);
    let parent = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())?
        .protect(
            &source,
            &children,
            &mut journal,
            &tig_episode::CompositionOwnership {
                coordination: &coordination,
                identity: identity(&t, "parent")?,
                expected_revision: 0,
                budget: &budget,
            },
            &phase(&t.source_id, &t.operation, "protect/parent")?,
        )?;
    if t.inputs.purpose == Purpose::VerifyComponents {
        check(&t, &mut journal)?;
        parent.verify_current(&source, &children, &mut journal)?;
        let result = json!({"schema":"reel.tig-episode-component-verification-result.experimental-1",
            "recipe":t.recipe,"resources":frozen.selection(),"metadata_checkpoint":checkpoint,
            "journal_checkpoint":journal.checkpoint(),"temporary_sources":temporary_sources(&t)?,
            "temporary_owner_cleanup_required":true,"components_verified":scenes.len()+presentations.len(),
            "component_primary_references_verified":true,"episode_assembled":false,
            "final_episode_output_verified":false,"record_contract":null,"production_claim":null,"approval":false});
        write_report(Path::new("worker.json"), &result)?;
        return Ok(result);
    }
    let archive: ArchiveProfile = serde_json::from_value(t.inputs.source.clone())?;
    let parent_plan = tig_episode::plan(&mut source, &t.inputs.request)?;
    let controls = tig_episode::ParentControls::open(archive.byte_source, &parent_plan.conform)?;
    check(&t, &mut journal)?;
    let rendered = parent.assemble(
        &source,
        &children,
        &mut journal,
        Path::new("native"),
        tig_episode::CompositionFiles {
            parent: &controls,
            scenes: &BTreeMap::new(),
            inputs: &BTreeMap::new(),
            outputs: &BTreeMap::new(),
        },
    )?;
    let actual = rendered
        .outputs()
        .keys()
        .map(String::as_str)
        .chain(std::iter::once("receipt.json"))
        .collect::<std::collections::BTreeSet<_>>();
    if !actual
        .iter()
        .copied()
        .eq(t.outputs.keys().map(String::as_str))
    {
        bail!("episode output contract differs from admission");
    }
    for (name, max) in &t.outputs {
        if fs::metadata(Path::new("native").join(name))?.len() > *max {
            bail!("episode output exceeds its admitted size");
        }
    }
    check(&t, &mut journal)?;
    let candidates = crate::tig_native_job_cli::records::emit_candidate(
        &templates,
        rendered.outputs(),
        rendered.receipt_object(),
        |bindings| rendered.claim(bindings),
    )?;
    let owners = temporary_sources(&t)?;
    let result = json!({"schema":"reel.tig-episode-worker-result.experimental-1","recipe":t.recipe,
        "resources":frozen.selection(),"receipt":rendered.receipt_object(),"metadata_checkpoint":checkpoint,
        "journal_checkpoint":journal.checkpoint(),"temporary_sources":owners,"temporary_owner_cleanup_required":true,
        "decoded_master_matches_ordered_segments":rendered.receipt().decoded_master_matches_ordered_segments,
        "timestamps_verified":rendered.receipt().timestamps_verified,"record_candidates":candidates,"record_contract":records::contract(&t.inputs)?,"production_claim":null,"approval":false});
    write_report(Path::new("worker.json"), &result)?;
    Ok(result)
}
