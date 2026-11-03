//! Source-owned browser presentation references over the shared job protocol.
use super::*;
use crate::{media_runtime::BoundTool, tig_browser, tig_browser_reference};

pub struct JobPlan<'a> {
    context: &'a ProductionContext,
    programs: &'a BoundPrograms,
    browser: &'a BoundTool,
    limits: &'a tig_browser::render::Limits,
    clean: bool,
    recipe: Plan,
    prepared: tig_browser::Prepared,
    runtime: BTreeMap<LogicalPath, Object>,
    evidence: BTreeMap<LogicalPath, Object>,
    composition: BTreeMap<LogicalPath, Object>,
    metadata: BTreeMap<Object, Vec<u8>>,
    target: Reference,
    master: FileRef,
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

fn target(
    source: &Source,
    request: &tig_browser::Request,
    id: &str,
    definition: &str,
) -> Result<(Reference, FileRef)> {
    let output = source.record(id)?;
    if output.definition().id != definition
        || request.root_definition == definition
        || request.primary_definitions.contains(definition)
        || !matches!(
            source.admission(definition)?.payload,
            crate::tig_inputs::Payload::Selected {}
        )
        || output.authored().get("presentation") != Some(&json!({"id":request.root_id}))
        || output.authored().get("component_kind") != Some(&json!("episode-presentation"))
    {
        bail!("browser presentation output differs from admitted source or type");
    }
    for name in ["language", "episode_id", "source_segment_id"] {
        let value = output
            .authored()
            .get(name)
            .and_then(Value::as_str)
            .context("browser presentation occurrence scope missing")?;
        token(value)?;
    }
    let selected = output.selected()?;
    Ok((
        output.reference().clone(),
        FileRef {
            path: id.into(),
            sha256: selected.sha256.as_str().into(),
            bytes: selected.bytes,
        },
    ))
}
impl<'a> JobPlan<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn from_context(
        source: &mut Source,
        request: &tig_browser::Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        browser: &'a BoundTool,
        limits: &'a tig_browser::render::Limits,
        clean: bool,
        output_id: &str,
        output_definition: &str,
    ) -> Result<Self> {
        limits.validate()?;
        let (target, master) = target(source, request, output_id, output_definition)?;
        let producer = crate::tig_browser::render::producer_document(programs, browser, clean)?;
        let producer_pin = Object::from_bytes(&producer);
        let recipe = tig_browser::recipe(source, request, context, &producer_pin)?;
        let prepared = tig_browser::lower(source, request)?;
        let mut metadata = BTreeMap::from([
            (producer_pin, producer),
            (
                Object::from_bytes(programs.manifest_bytes()),
                programs.manifest_bytes().to_vec(),
            ),
            (
                Object::from_bytes(&prepared.manifest_bytes),
                prepared.manifest_bytes.clone(),
            ),
        ]);
        let admission = identity::canonical_json(&json!({
            "schema":"reel.tig-browser-reference-admission.experimental-1",
            "request":request,"limits":limits,"clean_picture":clean,"browser":browser.identity()
        }))?;
        metadata.insert(Object::from_bytes(&admission), admission);
        let runtime = programs
            .components()
            .map(|(name, pin)| {
                Ok((
                    LogicalPath::parse(Digest::blob(name.as_bytes()).as_str())?,
                    object(pin)?,
                ))
            })
            .chain(std::iter::once(Ok((
                LogicalPath::parse("browser/executable")?,
                object(browser.identity())?,
            ))))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut evidence = metadata
            .keys()
            .map(|pin| {
                Ok((
                    LogicalPath::parse(&format!("interpretation/{}", pin.sha256.as_str()))?,
                    pin.clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        evidence.insert(
            LogicalPath::parse("selected-record")?,
            target.object.clone(),
        );
        let composition = BTreeMap::from([(
            LogicalPath::parse("selected-master")?,
            Object {
                sha256: Digest::parse(&master.sha256)?,
                bytes: master.bytes,
            },
        )]);
        Ok(Self {
            context,
            programs,
            browser,
            limits,
            clean,
            recipe,
            prepared,
            runtime,
            evidence,
            composition,
            metadata,
            target,
            master,
            definitions: request
                .primary_definitions
                .iter()
                .chain(std::iter::once(&request.root_definition))
                .cloned()
                .collect(),
        })
    }
    pub fn recipe(&self) -> &Plan {
        &self.recipe
    }
    pub fn selected_master(&self) -> &FileRef {
        &self.master
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
        for (pin, raw) in &self.metadata {
            if write {
                if store.put(raw)? != *pin {
                    bail!("browser interpretation publication differs");
                }
            } else if store.get(pin)?.as_deref() != Some(raw) {
                bail!("browser interpretation unavailable or changed");
            }
        }
        Ok(())
    }
    pub fn freeze(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        self.interpretation(store, true)?;
        Ok(job::freeze_with_dependencies(
            &self.admission(),
            &self.dependencies(),
            None,
            store,
            limits,
        )?)
    }
    pub fn readback<'p>(
        &'p self,
        selection: &Selection,
        store: &mut impl NodeStore,
        limits: TreeLimits,
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
                limits,
            )?,
        })
    }
}
impl<'p, 'a> Readback<'p, 'a> {
    pub fn resources(&self) -> &job::JobContext {
        &self.resources
    }
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
            bail!("browser presentation source or protection budget differs");
        }
        let owner = ScopedSource::protect(
            journal,
            coordination,
            identity,
            revision,
            operation,
            self.resources.selection(),
        )?;
        let protected = ProtectedPresentation {
            readback: self,
            source: owner,
        };
        protected.verify_current(source, journal)?;
        Ok(protected)
    }
}
impl ProtectedPresentation<'_, '_> {
    pub fn resources(&self) -> &job::JobContext {
        &self.readback.resources
    }
    pub fn revision(&self) -> u64 {
        self.source.revision()
    }
    pub fn verify_current(&self, source: &Source, journal: &mut SourceJournal) -> Result<()> {
        if source.checkpoint() != self.readback.plan.recipe.checkpoint() {
            bail!("browser presentation belongs to another source");
        }
        self.source.check(journal)
    }
    pub fn verify_selected_master<'p>(
        &'p self,
        source: &mut Source,
        journal: &mut SourceJournal,
        stage_parent: &std::path::Path,
        max_staged_bytes: u64,
    ) -> Result<VerifiedPresentation<'p>> {
        self.verify_current(source, journal)?;
        let plan = self.readback.plan;
        if plan.limits.max_workspace_bytes > max_staged_bytes {
            bail!("browser reference workspace exceeds admitted staging budget");
        }
        let inputs =
            source.prepare_files(&plan.prepared.inputs.values().cloned().collect::<Vec<_>>())?;
        let outputs = source.prepare_files(std::slice::from_ref(&plan.master))?;
        let compared = tig_browser_reference::compare_master_to_primaries(
            &plan.prepared,
            &inputs,
            plan.programs,
            plan.browser,
            plan.limits,
            plan.clean,
            &plan.master,
            &outputs,
            stage_parent,
            || self.verify_current(source, journal),
        );
        self.verify_current(source, journal)?;
        let compared = compared?;
        let (yuv, rgb) = compared.picture_sha256();
        Ok(VerifiedPresentation {
            owner: &self.source,
            _programs: plan.programs,
            _inputs: inputs,
            outputs,
            checkpoint: source.checkpoint().clone(),
            source: plan.prepared.source_record.clone(),
            output: plan.target.clone(),
            recipe: plan.recipe.fingerprint().clone(),
            manifest: FileRef {
                path: "browser-manifest.json".into(),
                sha256: compared.manifest().sha256.as_str().into(),
                bytes: compared.manifest().bytes,
            },
            master: compared.master().clone(),
            yuv: yuv.into(),
            rgb: Some(rgb.into()),
            retained_source: None,
            audio: compared.audio_sha256().into(),
            frames: compared.frames(),
            samples: compared.samples(),
            resources: &self.readback.resources,
            production_definitions: &plan.definitions,
        })
    }
}
