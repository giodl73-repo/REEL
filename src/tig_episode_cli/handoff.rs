//! Retain the full completed worker context before withdrawing temporary roots.
//! Metadata-only recovery; no rendering, payload deletion or approval authority.
use super::*;
use tig_core::tree::Tree;
const HEADER: &str = "reel.tig-episode-retained-context.experimental-1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Temporary {
    source: String,
    selection: Selection,
    revision: u64,
}
fn expected(p: &Profile) -> Result<Vec<String>> {
    p.inputs
        .scenes
        .iter()
        .map(|spec| format!("scene/{}", spec.id))
        .chain(
            p.inputs
                .presentations
                .iter()
                .map(|spec| format!("presentation/{}", spec.id)),
        )
        .chain(std::iter::once("parent".into()))
        .map(|name| {
            Ok(phase(&p.source_id, &p.operation, &name)?
                .as_str()
                .to_owned())
        })
        .collect()
}
fn entries(
    selection: &Selection,
    store: &mut impl NodeStore,
) -> Result<BTreeMap<LogicalPath, Object>> {
    Ok(Tree::from_root(selection.root.clone())
        .entries(store, TreeLimits::default())?
        .0)
}
fn document(store: &impl NodeStore, object: &Object) -> Result<Value> {
    if object.bytes > 65536 {
        bail!("episode handoff metadata document exceeds bound");
    }
    Ok(identity::parse_json(
        &store
            .get(object)?
            .context("episode handoff metadata missing")?,
    )?)
}
fn job_header(
    selection: &Selection,
    entries: &BTreeMap<LogicalPath, Object>,
    store: &impl NodeStore,
) -> Result<Value> {
    let header = entries
        .get(&LogicalPath::parse("job/identity")?)
        .context("episode job header missing")?;
    if header.sha256 != selection.context {
        bail!("episode job context identity differs");
    }
    document(store, header)
}
fn retain(
    objects: &mut BTreeMap<Digest, Object>,
    logical_bytes: &mut u64,
    object: &Object,
    budget: &ProtectionBudget,
) -> Result<()> {
    if let Some(old) = objects.get(&object.sha256) {
        if old.bytes != object.bytes {
            bail!("episode handoff resource lengths conflict");
        }
        return Ok(());
    }
    let next = logical_bytes
        .checked_add(object.bytes)
        .context("episode handoff byte overflow")?;
    if objects.len() >= budget.max_objects || next > budget.max_logical_bytes {
        bail!("episode handoff complete resource budget exceeded");
    }
    objects.insert(object.sha256.clone(), object.clone());
    *logical_bytes = next;
    Ok(())
}
fn read_report(p: &Profile, object: &Object) -> Result<Value> {
    use tig_store::existing::{ByteSource, Profile as CacheProfile, ReadCache};
    if object.bytes > 65536 {
        bail!("episode completion report exceeds bound");
    }
    let profile: CacheProfile = serde_json::from_value(
        json!({"places":p.cache.cache_roots.iter().map(|root|json!({"root":root,"layout":"reel-cas"})).collect::<Vec<_>>(),"required":"prevent-mutation"}),
    )?;
    let mut cache = ReadCache::new(profile)?;
    let mut guard = cache.open_verified(object)?;
    let mut raw = Vec::new();
    (&mut guard).take(65537).read_to_end(&mut raw)?;
    if raw.len() as u64 != object.bytes {
        bail!("episode completion report changed");
    }
    Ok(identity::parse_json(&raw)?)
}
pub(super) fn finish(p: &Profile, journal: &mut SourceJournal) -> Result<Value> {
    let coordination = Coordination {
        root: p
            .cache
            .coordination_root
            .clone()
            .context("episode coordination missing")?,
    };
    let identity = Identity {
        source: key(&p.source_id)?,
        owner: key(&p.owner)?,
    };
    let _root_guard = jobs::fence(&coordination, journal, &identity)?;
    let state = journal
        .source(&identity.source)?
        .context("episode result source missing")?;
    if state.owner != identity.owner || state.pending.is_some() {
        bail!("episode result owner or pending state differs");
    }
    let current = state
        .current
        .context("episode result source withdrawn")?
        .selection;
    let wait = MetadataWait::default();
    let mut reader = Waiting::new(
        wait.run(|| MetadataReader::open(&p.metadata_root, &p.metadata_minimum))?,
        &wait,
    );
    let retained = entries(&current, &mut reader)?;
    let expected = expected(p)?;
    let mut ordered = expected.clone();
    ordered.sort();
    let mut guards = Vec::new();
    for source in ordered {
        guards.push(jobs::fence(
            &coordination,
            journal,
            &Identity {
                source: key(&source)?,
                owner: identity.owner.clone(),
            },
        )?);
    }
    let (selection, temporary, metadata) = if let Some(header) =
        retained.get(&LogicalPath::parse("handoff/identity")?)
    {
        let header = document(&reader, header)?;
        if header["schema"] != HEADER
            || header["source_id"] != p.source_id
            || header["owner"] != p.owner
            || header["operation"] != p.operation
            || header["root_revision_after"].as_u64() != Some(state.revision)
        {
            bail!("episode retained context belongs to another attempt");
        }
        let temporary: Vec<Temporary> = serde_json::from_value(header["temporary"].clone())?;
        if !temporary.iter().map(|t| &t.source).eq(expected.iter()) {
            bail!("episode retained temporary scope differs");
        }
        (current, temporary, None)
    } else {
        let header_object = retained
            .get(&LogicalPath::parse("retention/identity")?)
            .context("episode runner retention header missing")?;
        let header = document(&reader, header_object)?;
        let started = p
            .expected_revision
            .checked_add(2)
            .context("episode revision overflow")?;
        let completed = started
            .checked_add(2)
            .context("episode revision overflow")?;
        if state.revision != completed
            || header["schema"] != "reel.tig-runner-retention.experimental-1"
            || header["source_id"] != p.source_id
            || header["owner"] != p.owner
            || header["revision"].as_u64() != Some(started)
            || header["succeeded"] != true
        {
            bail!("episode result is not the completed admitted attempt");
        }
        let input_selection: Selection = serde_json::from_value(header["source"].clone())?;
        let input_entries = entries(&input_selection, &mut reader)?;
        let input_header = job_header(&input_selection, &input_entries, &reader)?;
        if input_header["source"] != serde_json::to_value(&p.inputs.context)? {
            bail!("episode result source context differs");
        }
        let report = read_report(
            p,
            retained
                .get(&LogicalPath::parse(&format!(
                    "outputs/{}",
                    Digest::blob(b"worker.json").as_str()
                ))?)
                .context("episode completion report missing")?,
        )?;
        if report["schema"] != "reel.tig-episode-worker-result.experimental-1"
            || report["recipe"] != input_header["recipe"]
            || report["decoded_master_matches_ordered_segments"] != true
            || report["timestamps_verified"] != true
            || report["temporary_sources"] != json!(expected)
        {
            bail!("episode completion scope or native checks differ");
        }
        let parent_selection: Selection = serde_json::from_value(report["resources"].clone())?;
        let parent_entries = entries(&parent_selection, &mut reader)?;
        let parent_header = job_header(&parent_selection, &parent_entries, &reader)?;
        for field in [
            "source",
            "checkpoint",
            "admission",
            "binding",
            "consumption",
            "recipe",
            "operation",
        ] {
            if parent_header[field] != input_header[field] {
                bail!("episode parent context differs from admitted composition");
            }
        }
        let budget = protection(p.max_objects, p.max_logical_bytes)?;
        let mut objects = BTreeMap::new();
        let mut logical_bytes = 0;
        for object in retained
            .values()
            .chain(input_entries.values())
            .chain(parent_entries.values())
            .chain([&current.root, &input_selection.root, &parent_selection.root])
        {
            retain(&mut objects, &mut logical_bytes, object, &budget)?;
        }
        let mut temporary = Vec::new();
        for source in &expected {
            let child = journal
                .source(&key(source)?)?
                .context("episode temporary source missing")?;
            if child.owner != identity.owner || child.revision != 2 || child.pending.is_some() {
                bail!("episode temporary source changed before handoff");
            }
            let selection = child
                .current
                .context("episode temporary source withdrawn before handoff")?
                .selection;
            if source == expected.last().context("episode parent source missing")? {
                if selection != parent_selection {
                    bail!("episode parent selection differs from report");
                }
            } else if !input_entries
                .values()
                .any(|object| object == &selection.root)
            {
                bail!("episode child source was not in the declared input closure");
            }
            let nodes = entries(&selection, &mut reader)?;
            job_header(&selection, &nodes, &reader)?;
            for object in nodes.values().chain(std::iter::once(&selection.root)) {
                retain(&mut objects, &mut logical_bytes, object, &budget)?;
            }
            temporary.push(Temporary {
                source: source.clone(),
                selection,
                revision: 2,
            });
        }
        let next = state
            .revision
            .checked_add(2)
            .context("episode handoff revision overflow")?;
        let raw = identity::canonical_json(
            &json!({"schema":HEADER,"source_id":p.source_id,"owner":p.owner,"operation":p.operation,
            "previous":current,"root_revision_before":state.revision,"root_revision_after":next,"temporary":temporary,
            "native_production_claim":null,"approval":false,"resource_objects":objects.len()}),
        )?;
        if raw.len() > 65536 {
            bail!("episode handoff header exceeds bound");
        }
        let object = Object::from_bytes(&raw);
        let mut names = objects
            .into_values()
            .map(|object| {
                Ok((
                    LogicalPath::parse(&format!("objects/{}", object.sha256.as_str()))?,
                    object,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        names.insert(LogicalPath::parse("handoff/identity")?, object.clone());
        let mut writer = Waiting::new(
            wait.run(|| MetadataStore::open(&p.metadata_root, &p.metadata_minimum))?,
            &wait,
        );
        if writer.put(&raw)? != object {
            bail!("episode handoff header changed");
        }
        let tree = Tree::build(&names, &mut writer, TreeLimits::default())?.0;
        let witness = wait.run(|| writer.inner.finish())?;
        drop(writer);
        let mut reopened = Waiting::new(
            wait.run(|| MetadataReader::open(&p.metadata_root, &witness))?,
            &wait,
        );
        if Tree::from_root(tree.root().clone())
            .entries(&mut reopened, TreeLimits::default())?
            .0
            != names
            || reopened.get(&object)?.as_deref() != Some(raw.as_slice())
        {
            bail!("episode complete handoff readback differs");
        }
        let selection = Selection {
            root: tree.root().clone(),
            context: object.sha256,
        };
        let prepared = journal.apply(
            phase(&p.source_id, &p.operation, "handoff/prepare")?,
            Command::Prepare {
                source: identity.source.clone(),
                owner: identity.owner.clone(),
                expected_revision: state.revision,
                selection: selection.clone(),
            },
        )?;
        journal.apply(
            phase(&p.source_id, &p.operation, "handoff/adopt")?,
            Command::Adopt {
                source: identity.source.clone(),
                owner: identity.owner.clone(),
                expected_revision: prepared.source_revision,
                prepared: prepared.reference,
            },
        )?;
        let active = journal
            .source(&identity.source)?
            .context("episode handoff source missing")?;
        if active.owner != identity.owner
            || active.revision != next
            || active.pending.is_some()
            || active
                .current
                .as_ref()
                .is_none_or(|state| state.selection != selection)
        {
            bail!("episode handoff no longer current");
        }
        (selection, temporary, Some(witness))
    };
    // This loop is reached only after complete context publication, reopening
    // and root adoption. Failure leaves remaining roots live; finish can resume.
    for temp in &temporary {
        let source = key(&temp.source)?;
        let state = journal
            .source(&source)?
            .context("episode temporary source missing during retirement")?;
        if state.owner != identity.owner || state.pending.is_some() {
            bail!("episode temporary owner changed");
        }
        if state.current.is_none() && state.revision == temp.revision + 1 {
            continue;
        }
        if state.revision != temp.revision
            || state
                .current
                .as_ref()
                .is_none_or(|state| state.selection != temp.selection)
        {
            bail!("episode temporary selection changed");
        }
        journal.apply(
            phase(
                &p.source_id,
                &p.operation,
                &format!("handoff/withdraw/{}", temp.source),
            )?,
            Command::Withdraw {
                source,
                owner: identity.owner.clone(),
                expected_revision: temp.revision,
            },
        )?;
    }
    drop(guards);
    Ok(
        json!({"schema":HEADER,"selection":selection,"sources_withdrawn":temporary.len(),"temporary_owner_cleanup_required":false,
        "metadata_checkpoint":metadata,"journal_checkpoint":journal.checkpoint(),"approval":false,"authorizes_eviction":false}),
    )
}

/// Resume a completed job's metadata handoff without rendering or binding an
/// executable. Refuses failed/changed jobs and preserves unfinished obligations.
pub fn finish_episode(path: &Path) -> Result<Value> {
    let p: Profile = read(path, 128 * 1024, None)?;
    validate(&p)?;
    let mut journal = SourceJournal::open(&p.journal_root, &p.journal_minimum)?;
    finish(&p, &mut journal)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_resource_accounting_deduplicates_and_preserves_state_on_refusal() {
        let budget = ProtectionBudget {
            max_objects: 1,
            max_logical_bytes: 3,
        };
        let object = Object::from_bytes(b"abc");
        let mut objects = BTreeMap::new();
        let mut bytes = 0;
        retain(&mut objects, &mut bytes, &object, &budget).unwrap();
        retain(&mut objects, &mut bytes, &object, &budget).unwrap();
        assert_eq!(bytes, 3);
        assert_eq!(objects.len(), 1);
        let conflict = Object {
            sha256: object.sha256.clone(),
            bytes: 4,
        };
        assert!(retain(&mut objects, &mut bytes, &conflict, &budget).is_err());
        assert!(
            retain(
                &mut objects,
                &mut bytes,
                &Object::from_bytes(b"new"),
                &budget
            )
            .is_err()
        );
        assert_eq!(objects.get(&object.sha256), Some(&object));
        assert_eq!(bytes, 3);
    }
}
