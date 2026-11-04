//! Owned exact-byte references to delivered outputs, distinct from primaries.
use super::*;
use crate::{presentation_adopt, tig_retained_presentation as adapter};

pub struct JobPlan<'a> {
    context: &'a ProductionContext,
    programs: &'a BoundPrograms,
    recipe: Plan,
    prepared: adapter::Prepared,
    target: Reference,
    master: FileRef,
    runtime: BTreeMap<LogicalPath, Object>,
    evidence: BTreeMap<LogicalPath, Object>,
    composition: BTreeMap<LogicalPath, Object>,
    definitions: BTreeSet<String>,
}
pub struct Readback<'p, 'a> {
    plan: &'p JobPlan<'a>,
    resources: job::JobContext,
}
pub struct ProtectedPresentation<'p, 'a> {
    readback: Readback<'p, 'a>,
    source: ScopedSource,
}
impl<'a> JobPlan<'a> {
    pub fn from_context(
        source: &mut Source,
        request: &adapter::Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        output_id: &str,
        output_definition: &str,
    ) -> Result<Self> {
        let prepared = adapter::lower(source, request)?;
        let output = source.record(output_id)?;
        let authored = output.authored();
        if output.definition().id != output_definition
            || output_definition == request.root_definition
            || request.source_definitions.contains(output_definition)
            || !matches!(
                source.admission(output_definition)?.payload,
                crate::tig_inputs::Payload::Selected {}
            )
            || authored.get("presentation") != Some(&json!({"id":request.root_id}))
            || authored.get("component_kind") != Some(&json!("episode-presentation"))
            || authored.get("episode_id") != Some(&json!(prepared.manifest.episode_id))
            || authored.get("source_segment_id") != Some(&json!(prepared.manifest.role))
            || authored.get("language") != Some(&json!(prepared.manifest.language))
            || output.selected()?.sha256.as_str() != prepared.source_file.sha256
            || output.selected()?.bytes != prepared.source_file.bytes
        {
            bail!("retained occurrence differs from source, type, scope or exact bytes");
        }
        if let Some(scope) = authored.get("presentation_scope") {
            let scope: SourceScope = serde_json::from_value(scope.clone())?;
            if scope.episode_id != prepared.manifest.episode_id
                || scope.role != prepared.manifest.role
            {
                bail!("retained occurrence scope differs");
            }
        }
        let target = output.reference().clone();
        let master = FileRef {
            path: output_id.into(),
            sha256: prepared.source_file.sha256.clone(),
            bytes: prepared.source_file.bytes,
        };
        let recipe = adapter::recipe(
            source,
            request,
            context,
            &Object::from_bytes(programs.manifest_bytes()),
        )?;
        let runtime = programs
            .components()
            .map(|(name, pin)| {
                Ok((
                    LogicalPath::parse(Digest::blob(name.as_bytes()).as_str())?,
                    object(pin)?,
                ))
            })
            .collect::<Result<_>>()?;
        let mut evidence = prepared
            .metadata()
            .map(|(pin, _)| {
                Ok((
                    LogicalPath::parse(&format!("controls/{}", pin.sha256.as_str()))?,
                    pin.clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        evidence.insert(
            LogicalPath::parse("source-record")?,
            prepared.source.object.clone(),
        );
        evidence.insert(LogicalPath::parse("target-record")?, target.object.clone());
        let selected = Object {
            sha256: Digest::parse(&master.sha256)?,
            bytes: master.bytes,
        };
        let composition = BTreeMap::from([
            (
                LogicalPath::parse("source-delivered-output")?,
                selected.clone(),
            ),
            (LogicalPath::parse("selected-occurrence")?, selected),
        ]);
        Ok(Self {
            context,
            programs,
            recipe,
            prepared,
            target,
            master,
            runtime,
            evidence,
            composition,
            definitions: BTreeSet::from([request.root_definition.clone()]),
        })
    }
    fn admission(&self) -> job::Admission<'_> {
        job::Admission {
            context: self.context,
            plan: &self.recipe,
            runtime: &self.runtime,
            output: None,
        }
    }
    fn dependencies(&self) -> job::Dependencies<'_> {
        job::Dependencies {
            composition: &self.composition,
            evidence: &self.evidence,
        }
    }
    fn interpretation(&self, store: &mut impl NodeStore, write: bool) -> Result<()> {
        for (pin, raw) in self.prepared.metadata().chain(std::iter::once((
            &self.recipe.operation().producer,
            self.programs.manifest_bytes(),
        ))) {
            if write {
                if store.put(raw)? != *pin {
                    bail!("retained interpretation publication differs");
                }
            } else if store.get(pin)?.as_deref() != Some(raw) {
                bail!("retained interpretation unavailable or changed");
            }
        }
        Ok(())
    }
    pub fn freeze(&self, store: &mut impl NodeStore, trees: TreeLimits) -> Result<job::JobContext> {
        self.interpretation(store, true)?;
        Ok(job::freeze_with_dependencies(
            &self.admission(),
            &self.dependencies(),
            None,
            store,
            trees,
        )?)
    }
    pub fn readback<'p>(
        &'p self,
        selection: &Selection,
        store: &mut impl NodeStore,
        trees: TreeLimits,
    ) -> Result<Readback<'p, 'a>> {
        self.interpretation(store, false)?;
        Ok(Readback {
            plan: self,
            resources: job::restore_with_dependencies(
                selection,
                &self.admission(),
                &self.dependencies(),
                None,
                store,
                trees,
            )?,
        })
    }
}
impl<'p, 'a> Readback<'p, 'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn protect(
        self,
        source: &Source,
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        revision: u64,
        operation: &Key,
        budget: &ProtectionBudget,
    ) -> Result<ProtectedPresentation<'p, 'a>> {
        if source.checkpoint() != self.plan.recipe.checkpoint()
            || budget.max_objects == 0
            || budget.max_objects > 1_000_000
            || budget.max_logical_bytes == 0
            || budget.max_logical_bytes > 9_007_199_254_740_991
            || self.resources.objects().len() > budget.max_objects
            || self.resources.logical_bytes() > budget.max_logical_bytes
        {
            bail!("retained presentation protection exceeds scope or bounds");
        }
        let owner = ScopedSource::protect(
            journal,
            coordination,
            identity,
            revision,
            operation,
            self.resources.selection(),
        )?;
        let result = ProtectedPresentation {
            readback: self,
            source: owner,
        };
        result.verify_current(source, journal)?;
        Ok(result)
    }
}
struct DeliveredFiles<'a> {
    prepared: &'a adapter::Prepared,
    inputs: &'a crate::tig_inputs::Inputs,
}
impl InputFiles for DeliveredFiles<'_> {
    fn verified_path(&self, file: &FileRef) -> Result<std::path::PathBuf> {
        if file != &self.prepared.manifest.source {
            bail!("unbound delivered source alias");
        }
        self.inputs.verified_path(&self.prepared.source_file)
    }
}
impl ProtectedPresentation<'_, '_> {
    fn verify_current(&self, source: &Source, journal: &mut SourceJournal) -> Result<()> {
        let plan = self.readback.plan;
        if source.checkpoint() != plan.recipe.checkpoint()
            || source.record(&plan.prepared.root.id)?.reference() != &plan.prepared.root
            || source.record(&plan.prepared.source.id)?.reference() != &plan.prepared.source
            || source.record(&plan.target.id)?.reference() != &plan.target
        {
            bail!("retained source changed");
        }
        self.source.check(journal)
    }
    pub fn verify_selected_master<'p>(
        &'p self,
        source: &mut Source,
        journal: &mut SourceJournal,
        stage: &std::path::Path,
        budget: u64,
    ) -> Result<VerifiedPresentation<'p>> {
        self.verify_current(source, journal)?;
        let plan = self.readback.plan;
        if plan
            .master
            .bytes
            .checked_add(65_536)
            .is_none_or(|bytes| bytes > budget)
        {
            bail!("retained reference exceeds admitted staging budget");
        }
        let inputs = source.prepare_files(std::slice::from_ref(&plan.prepared.source_file))?;
        let outputs = source.prepare_files(std::slice::from_ref(&plan.master))?;
        let files = DeliveredFiles {
            prepared: &plan.prepared,
            inputs: &inputs,
        };
        let programs = plan.programs.with_inputs(&files);
        std::fs::create_dir_all(stage)?;
        let temporary = tempfile::Builder::new()
            .prefix(".retained-reference-")
            .tempdir_in(stage)?;
        let result = presentation_adopt::build_with_control_files(
            &plan.prepared.manifest_file,
            &plan.prepared,
            &programs,
            &temporary.path().join("reference"),
        );
        self.verify_current(source, journal)?;
        let receipt = result?;
        if receipt.master_sha256 != plan.master.sha256
            || receipt.master_bytes != plan.master.bytes
            || receipt.output_mode != Some(presentation_adopt::OutputMode::RetainSource)
        {
            bail!("retained comparison changed exact bytes");
        }
        // The same exact-byte source and occurrence guards stay alive through
        // parent use. No original upstream production claim is manufactured.
        Ok(VerifiedPresentation {
            owner: &self.source,
            _programs: plan.programs,
            _inputs: inputs,
            outputs,
            checkpoint: source.checkpoint().clone(),
            source: plan.prepared.root.clone(),
            output: plan.target.clone(),
            recipe: plan.recipe.fingerprint().clone(),
            manifest: plan.prepared.manifest_file.clone(),
            master: plan.master.clone(),
            yuv: receipt.decoded_picture_sha256,
            rgb: None,
            audio: receipt.decoded_audio_sha256,
            retained_source: Some(plan.prepared.source.clone()),
            geometry_admission: None,
            frames: receipt.frames,
            samples: receipt.samples,
            resources: &self.readback.resources,
            production_definitions: &plan.definitions,
        })
    }
}
