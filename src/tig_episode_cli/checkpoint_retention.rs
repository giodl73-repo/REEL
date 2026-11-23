//! Retain episode recovery evidence under a separate, deterministic source.
use super::*;
use tig_core::tree::Tree;

pub(super) fn retain(profile: &Path, inspection: &Path) -> Result<Value> {
    let p: Profile = read(profile, 128 * 1024, None)?;
    validate(&p)?;
    let requested: ResumeInspection = read(inspection, 65536, None)?;
    let coordination = Coordination {
        root: p
            .cache
            .coordination_root
            .clone()
            .context("episode coordination required")?,
    };
    let mut journal = SourceJournal::open(&p.journal_root, &p.journal_minimum)?;
    let original = Identity {
        source: key(&p.source_id)?,
        owner: key(&p.owner)?,
    };
    let _fence = jobs::fence(&coordination, &journal, &original)?;
    let reviewed = describe_resume(profile, inspection)?;
    let (cache, workspace, _) = runner::quiescent_workspace(&p.cache, &requested.workspace)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let directory =
        runner::open_workspace_directory(&workspace).map_err(|error| anyhow::anyhow!("{error}"))?;
    let ticket: Ticket = read(
        &workspace.join("episode-ticket.json"),
        65536,
        Some(&requested.ticket),
    )?;
    worker::check(&ticket, &mut journal)?;

    let descriptor = identity::canonical_json(&reviewed["descriptor"])?;
    let descriptor_pin = Object::from_bytes(&descriptor);
    let files: BTreeMap<String, Object> =
        serde_json::from_value(reviewed["descriptor"]["files"].clone())?;
    let marker_path = reviewed["marker"]["path"]
        .as_str()
        .context("missing checkpoint marker name")?;
    let marker = reviewed["marker"]["utf8"]
        .as_str()
        .context("missing checkpoint marker bytes")?;
    let bytes = files
        .values()
        .try_fold(descriptor_pin.bytes, |total, pin| {
            total
                .checked_add(pin.bytes)
                .context("episode checkpoint size overflow")
        })?;
    if bytes > p.scratch_bytes {
        bail!("episode checkpoint exceeds the original scratch allowance");
    }
    let _reservation = reel_assets::admission::reserve_on(&p.journal_root, &p.cache, cache, bytes)
        .map_err(|error| anyhow::anyhow!("episode checkpoint publication admission: {error}"))?;
    for (name, pin) in &files {
        if name == marker_path {
            publish(&p, cache, pin, &mut marker.as_bytes())?;
        } else {
            let relative = Path::new(name);
            if relative.is_absolute()
                || !relative
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
            {
                bail!("episode checkpoint publication path is not workspace relative");
            }
            let path = directory.path().join(relative);
            if path.canonicalize()? != path || !fs::symlink_metadata(&path)?.is_file() {
                bail!("episode checkpoint publication path changed");
            }
            let mut file = fs::File::open(path)?;
            publish(&p, cache, pin, &mut file)?;
        }
    }
    publish(&p, cache, &descriptor_pin, &mut descriptor.as_slice())?;
    worker::check(&ticket, &mut journal)?;

    let mut store = MetadataStore::open(&p.metadata_root, &ticket.metadata_minimum)?;
    let mut entries = BTreeMap::from([
        (
            LogicalPath::parse("checkpoint/descriptor")?,
            descriptor_pin.clone(),
        ),
        (
            LogicalPath::parse("original/context")?,
            ticket.resources.root.clone(),
        ),
    ]);
    for (index, pin) in files.into_values().enumerate() {
        entries.insert(
            LogicalPath::parse(&format!("checkpoint/files/{index:05}"))?,
            pin,
        );
    }
    let root = Tree::build(&entries, &mut store, TreeLimits::default())?.0;
    let metadata = store.finish()?;
    drop(store);
    let mut reader = MetadataReader::open(&p.metadata_root, &metadata)?;
    if root.entries(&mut reader, TreeLimits::default())?.0 != entries {
        bail!("episode checkpoint retention tree differs on readback");
    }
    let selection = Selection {
        root: root.root().clone(),
        context: descriptor_pin.sha256.clone(),
    };
    let source = phase(&p.source_id, &p.operation, descriptor_pin.sha256.as_str())?;
    let prepared = journal.apply(
        phase(source.as_str(), &p.operation, "prepare")?,
        Command::Prepare {
            source: source.clone(),
            owner: original.owner.clone(),
            expected_revision: 0,
            selection: selection.clone(),
        },
    )?;
    let adopted = journal.apply(
        phase(source.as_str(), &p.operation, "adopt")?,
        Command::Adopt {
            source: source.clone(),
            owner: original.owner.clone(),
            expected_revision: prepared.source_revision,
            prepared: prepared.reference,
        },
    )?;
    let state = journal
        .source(&source)?
        .context("episode checkpoint owner missing")?;
    if state.owner != original.owner
        || state.revision != adopted.source_revision
        || state.pending.is_some()
        || state
            .current
            .as_ref()
            .is_none_or(|current| current.selection != selection)
    {
        bail!("episode checkpoint owner changed during retention");
    }
    worker::check(&ticket, &mut journal)?;
    Ok(json!({
        "schema":"reel.retained-native-episode-checkpoint.v1",
        "source_id":source,
        "source_revision":state.revision,
        "selection":selection,
        "descriptor":descriptor_pin,
        "metadata_checkpoint":metadata,
        "journal_checkpoint":journal.checkpoint(),
        "workspace_retained":true
    }))
}

fn publish(p: &Profile, cache: &Path, pin: &Object, reader: &mut dyn Read) -> Result<()> {
    let sha = pin.sha256.as_str();
    if reel_assets::active_existing(&p.cache, sha, pin.bytes)
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .is_none()
    {
        reel_assets::recover_from_reader(&p.journal_root, reader, cache, sha, pin.bytes)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
    }
    reel_assets::active_existing(&p.cache, sha, pin.bytes)
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .context("retained episode checkpoint object could not be reopened")?;
    Ok(())
}

#[derive(Deserialize)]
struct RetainedCheckpoint {
    schema: String,
    source_id: Key,
    source_revision: u64,
    selection: Selection,
    descriptor: Object,
    metadata_checkpoint: Checkpoint,
}

struct ResumeResources<'a> {
    original: &'a job::JobContext,
    evidence: std::collections::BTreeSet<Object>,
}
impl crate::tig_job_runner::ResourceView for ResumeResources<'_> {
    fn selection(&self) -> &Selection {
        self.original.selection()
    }

    fn of(&self, role: job::Role) -> &std::collections::BTreeSet<Object> {
        if role == job::Role::Evidence {
            &self.evidence
        } else {
            self.original.of(role)
        }
    }
}

pub(super) fn resume(profile: &Path, inspection: &Path, retained: &Path) -> Result<Value> {
    let p: Profile = read(profile, 128 * 1024, None)?;
    validate(&p)?;
    let requested: ResumeInspection = read(inspection, 65536, None)?;
    let checkpoint: RetainedCheckpoint = read(retained, 65536, None)?;
    if checkpoint.schema != "reel.retained-native-episode-checkpoint.v1" {
        bail!("unsupported episode checkpoint retention receipt");
    }
    let reviewed = describe_resume(profile, inspection)?;
    let descriptor_bytes = identity::canonical_json(&reviewed["descriptor"])?;
    if Object::from_bytes(&descriptor_bytes) != checkpoint.descriptor
        || checkpoint.source_id
            != phase(
                &p.source_id,
                &p.operation,
                checkpoint.descriptor.sha256.as_str(),
            )?
        || checkpoint.selection.context != checkpoint.descriptor.sha256
    {
        bail!("retained episode checkpoint differs from inspected prefix");
    }
    let (_, workspace, _) = runner::quiescent_workspace(&p.cache, &requested.workspace)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let _directory =
        runner::open_workspace_directory(&workspace).map_err(|error| anyhow::anyhow!("{error}"))?;
    let request = workspace.join("job.json");
    let _: Value = read(&request, 256 * 1024, Some(&requested.request))?;
    let ticket: Ticket = read(
        &workspace.join("episode-ticket.json"),
        65536,
        Some(&requested.ticket),
    )?;
    let mut journal = SourceJournal::open(&p.journal_root, &ticket.journal_minimum)?;
    let coordination = Coordination {
        root: ticket.coordination_root.clone(),
    };
    let identity = Identity {
        source: key(&p.source_id)?,
        owner: key(&p.owner)?,
    };
    let checkpoint_identity = Identity {
        source: checkpoint.source_id.clone(),
        owner: identity.owner.clone(),
    };
    let _checkpoint_fence = jobs::fence(&coordination, &journal, &checkpoint_identity)?;
    let state = journal
        .source(&checkpoint.source_id)?
        .context("retained episode checkpoint owner missing")?;
    if state.owner != identity.owner
        || state.revision != checkpoint.source_revision
        || state.pending.is_some()
        || state
            .current
            .as_ref()
            .is_none_or(|current| current.selection != checkpoint.selection)
    {
        bail!("retained episode checkpoint owner changed");
    }
    worker::check(&ticket, &mut journal)?;
    let files: BTreeMap<String, Object> =
        serde_json::from_value(reviewed["descriptor"]["files"].clone())?;
    let mut expected = BTreeMap::from([
        (
            LogicalPath::parse("checkpoint/descriptor")?,
            checkpoint.descriptor.clone(),
        ),
        (
            LogicalPath::parse("original/context")?,
            ticket.resources.root.clone(),
        ),
    ]);
    for (index, pin) in files.values().enumerate() {
        expected.insert(
            LogicalPath::parse(&format!("checkpoint/files/{index:05}"))?,
            pin.clone(),
        );
    }
    let mut reader = MetadataReader::open(&p.metadata_root, &checkpoint.metadata_checkpoint)?;
    if Tree::from_root(checkpoint.selection.root.clone())
        .entries(&mut reader, TreeLimits::default())?
        .0
        != expected
    {
        bail!("retained checkpoint has a different exact object roster");
    }
    let wait = MetadataWait::default();
    let programs = BoundPrograms::bind(&ticket.inputs.runtime)?;
    let (mut source, context) = restore(
        &ticket.inputs,
        &ticket.metadata_root,
        &ticket.metadata_minimum,
        &wait,
    )?;
    let preparation = Preparation::open_on(
        &p.cache,
        &p.journal_root,
        &p.preparation_cache,
        2 * (ticket.inputs.scenes.len() + ticket.inputs.presentations.len()) as u64 + 2,
    )?;
    let mut store = Waiting::new(
        MetadataStore::open(&p.metadata_root, &ticket.metadata_minimum)?,
        &wait,
    );
    let root = planning::freeze(
        &ticket.inputs,
        &mut source,
        &context,
        &programs,
        &mut store,
        preparation.path(),
    )?;
    if root.recipe.fingerprint() != &ticket.recipe
        || root.resources.selection() != &ticket.resources
    {
        bail!("resumed episode input closure differs from its original ticket");
    }
    admit(
        &root.resources,
        &protection(ticket.max_objects, ticket.max_logical_bytes)?,
    )?;
    let minimum = store.inner.finish()?;
    drop(store);
    let mut reader = Waiting::new(MetadataReader::open(&p.metadata_root, &minimum)?, &wait);
    planning::readback(
        &ticket.inputs,
        &source,
        &context,
        &programs,
        &root,
        &mut reader,
    )?;
    preparation.finish()?;
    let mut evidence = root.resources.of(job::Role::Evidence).clone();
    evidence.extend(files.into_values());
    evidence.insert(checkpoint.descriptor.clone());
    let resources = ResumeResources {
        original: &root.resources,
        evidence,
    };
    let descriptor = reel_assets::active_existing(
        &p.cache,
        checkpoint.descriptor.sha256.as_str(),
        checkpoint.descriptor.bytes,
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?
    .context("retained episode checkpoint descriptor is missing")?;
    let result = crate::tig_job_runner::resume_with_dependencies(
        &p.journal_root,
        &request,
        &resources,
        &mut journal,
        &crate::tig_job_runner::Profile {
            config: &p.cache,
            coordination: &coordination,
            identity,
            expected_revision: ticket.root_revision,
            operation: phase(&p.source_id, &p.operation, "retain")?,
            metadata_root: &p.metadata_root,
            metadata_minimum: &minimum,
            tree_limits: TreeLimits::default(),
        },
        (&descriptor, &requested.request),
    );
    finish_run(
        &p,
        Purpose::Assemble,
        &mut journal,
        result,
        requested.request,
        requested.ticket,
    )
}
