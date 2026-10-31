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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prepared: Option<source_claims::Reference>,
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
    finish_after_prepare(p, journal, || Ok(()))
}
fn finish_after_prepare(
    p: &Profile,
    journal: &mut SourceJournal,
    after_prepare: impl FnOnce() -> Result<()>,
) -> Result<Value> {
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
    if state.owner != identity.owner {
        bail!("episode result owner differs");
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
    let (selection, temporary, metadata, succeeded) = if let Some(header) =
        retained.get(&LogicalPath::parse("handoff/identity")?)
    {
        if state.pending.is_some() || header.sha256 != current.context {
            bail!("episode retained context identity or pending state differs");
        }
        let header = document(&reader, header)?;
        if header["schema"] != HEADER
            || header["source_id"] != p.source_id
            || header["owner"] != p.owner
            || header["operation"] != p.operation
            || header["root_revision_after"].as_u64() != Some(state.revision)
            || header["record_contract"] != serde_json::to_value(records::contract(&p.inputs)?)?
        {
            bail!("episode retained context belongs to another attempt");
        }
        let temporary: Vec<Temporary> = serde_json::from_value(header["temporary"].clone())?;
        let succeeded = header.get("succeeded").map_or(Ok(true), |value| {
            value.as_bool().context("episode handoff outcome missing")
        })?;
        let present: std::collections::BTreeSet<_> = temporary.iter().map(|t| &t.source).collect();
        if present.len() != temporary.len()
            || !temporary.iter().map(|t| &t.source).eq(expected
                .iter()
                .filter(|id| succeeded || present.contains(id)))
            || header
                .get("expected_temporary")
                .is_some_and(|value| value != &json!(expected))
        {
            bail!("episode retained temporary scope differs");
        }
        if !succeeded {
            for source in expected.iter().filter(|id| !present.contains(id)) {
                if journal.source(&key(source)?)?.is_some() {
                    bail!("episode absent temporary source changed");
                }
            }
        }
        if succeeded && !p.inputs.record_outputs.is_empty() {
            let previous: Selection = serde_json::from_value(header["previous"].clone())?;
            let original = entries(&previous, &mut reader)?;
            for object in original.values().chain(std::iter::once(&previous.root)) {
                if retained.get(&LogicalPath::parse(&format!(
                    "objects/{}",
                    object.sha256.as_str()
                ))?) != Some(object)
                {
                    bail!("episode candidate result is outside retained handoff");
                }
            }
            let identity = document(
                &reader,
                original
                    .get(&LogicalPath::parse("retention/identity")?)
                    .context("episode original result header missing")?,
            )?;
            let inputs: Selection = serde_json::from_value(identity["source"].clone())?;
            records::verify_completed(p, journal, &previous, &inputs, &original, &mut reader)?;
        }
        (current, temporary, None, succeeded)
    } else {
        let header_object = retained
            .get(&LogicalPath::parse("retention/identity")?)
            .context("episode runner retention header missing")?;
        if header_object.sha256 != current.context {
            bail!("episode runner retention context identity differs");
        }
        let header = document(&reader, header_object)?;
        let started = p
            .expected_revision
            .checked_add(2)
            .context("episode revision overflow")?;
        let completed = started
            .checked_add(2)
            .context("episode revision overflow")?;
        let interrupted = completed
            .checked_add(1)
            .context("episode revision overflow")?;
        if state.revision
            != if state.pending.is_some() {
                interrupted
            } else {
                completed
            }
            || header["schema"] != "reel.tig-runner-retention.experimental-1"
            || header["source_id"] != p.source_id
            || header["owner"] != p.owner
            || header["revision"].as_u64() != Some(started)
        {
            bail!("episode result is not the completed admitted attempt");
        }
        let succeeded = header["succeeded"]
            .as_bool()
            .context("episode runner outcome missing")?;
        if !succeeded
            && ["result/request", "result/evidence"]
                .iter()
                .any(|name| !retained.contains_key(&LogicalPath::parse(name).expect("fixed path")))
        {
            bail!("episode failed result diagnostics missing");
        }
        let input_selection: Selection = serde_json::from_value(header["source"].clone())?;
        let input_entries = entries(&input_selection, &mut reader)?;
        let input_header = job_header(&input_selection, &input_entries, &reader)?;
        if input_header["source"] != serde_json::to_value(&p.inputs.context)? {
            bail!("episode result source context differs");
        }
        let parent_selection = if succeeded {
            records::verify_completed(
                p,
                journal,
                &current,
                &input_selection,
                &retained,
                &mut reader,
            )?;
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
                || report["record_contract"] != serde_json::to_value(records::contract(&p.inputs)?)?
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
            Some(parent_selection)
        } else {
            None
        };
        let budget = protection(p.max_objects, p.max_logical_bytes)?;
        let mut objects = BTreeMap::new();
        let mut logical_bytes = 0;
        for object in retained
            .values()
            .chain(input_entries.values())
            .chain([&current.root, &input_selection.root])
        {
            retain(&mut objects, &mut logical_bytes, object, &budget)?;
        }
        let mut temporary = Vec::new();
        for source in &expected {
            let Some(child) = journal.source(&key(source)?)? else {
                if succeeded {
                    bail!("episode temporary source missing");
                }
                continue;
            };
            let prepared = child
                .pending
                .as_ref()
                .map(|pending| pending.reference.clone());
            let selection = match (&child.current, &child.pending) {
                (Some(current), None) if child.revision == 2 => current.selection.clone(),
                (None, Some(pending)) if !succeeded && child.revision == 1 => {
                    pending.selection.clone()
                }
                _ => bail!("episode temporary source changed before handoff"),
            };
            if child.owner != identity.owner {
                bail!("episode temporary source changed before handoff");
            }
            if let Some(reference) = &prepared {
                let label = p
                    .inputs
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
                    .find(|label| {
                        phase(&p.source_id, &p.operation, label)
                            .is_ok_and(|id| id.as_str() == source)
                    })
                    .context("episode pending component scope missing")?;
                let worker_operation =
                    phase(&p.source_id, &p.operation, &format!("protect/{label}"))?;
                let acknowledged = SourceJournal::acknowledge(
                    &p.journal_root,
                    &p.journal_minimum,
                    jobs::phase(
                        &Identity {
                            source: key(source)?,
                            owner: identity.owner.clone(),
                        },
                        &worker_operation,
                        "prepare",
                    )?,
                    Command::Prepare {
                        source: key(source)?,
                        owner: identity.owner.clone(),
                        expected_revision: 0,
                        selection: selection.clone(),
                    },
                )?;
                if &acknowledged.reference != reference {
                    bail!("episode pending component preparation differs");
                }
            }
            if source == expected.last().context("episode parent source missing")? {
                if parent_selection
                    .as_ref()
                    .is_some_and(|parent| &selection != parent)
                {
                    bail!("episode parent selection differs from report");
                }
            } else if !input_entries
                .values()
                .any(|object| object == &selection.root)
            {
                bail!("episode child source was not in the declared input closure");
            }
            let nodes = entries(&selection, &mut reader)?;
            let header = job_header(&selection, &nodes, &reader)?;
            if source == expected.last().context("episode parent source missing")? {
                for field in [
                    "source",
                    "checkpoint",
                    "admission",
                    "binding",
                    "consumption",
                    "recipe",
                    "operation",
                ] {
                    if header[field] != input_header[field] {
                        bail!("episode parent context differs from admitted composition");
                    }
                }
            }
            for object in nodes.values().chain(std::iter::once(&selection.root)) {
                retain(&mut objects, &mut logical_bytes, object, &budget)?;
            }
            temporary.push(Temporary {
                source: source.clone(),
                selection,
                revision: child.revision,
                prepared,
            });
        }
        let next = completed
            .checked_add(2)
            .context("episode handoff revision overflow")?;
        let raw = identity::canonical_json(
            &json!({"schema":HEADER,"source_id":p.source_id,"owner":p.owner,"operation":p.operation,
            "previous":current,"root_revision_before":completed,"root_revision_after":next,"temporary":temporary,"expected_temporary":expected,"succeeded":succeeded,
            "record_contract":records::contract(&p.inputs)?,"native_production_claim":null,"approval":false,"resource_objects":objects.len()}),
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
        // Rebuild/reopen the entire packet on recovery. Only the identical
        // pending selection and exact previously committed prepare may resume.
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.selection != selection)
        {
            bail!("episode pending handoff selection differs");
        }
        let prepared = journal.apply(
            phase(&p.source_id, &p.operation, "handoff/prepare")?,
            Command::Prepare {
                source: identity.source.clone(),
                owner: identity.owner.clone(),
                expected_revision: completed,
                selection: selection.clone(),
            },
        )?;
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.reference != prepared.reference)
        {
            bail!("episode pending handoff preparation differs");
        }
        after_prepare()?;
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
        (selection, temporary, Some(witness), succeeded)
    };
    // This loop is reached only after complete context publication, reopening
    // and root adoption. Failure leaves remaining roots live; finish can resume.
    for temp in &temporary {
        let source = key(&temp.source)?;
        let state = journal
            .source(&source)?
            .context("episode temporary source missing during retirement")?;
        if state.owner != identity.owner {
            bail!("episode temporary owner changed");
        }
        if state.current.is_none() && state.pending.is_none() && state.revision == temp.revision + 1
        {
            continue;
        }
        let matches = match (&temp.prepared, &state.current, &state.pending) {
            (None, Some(current), None) => current.selection == temp.selection,
            (Some(reference), None, Some(pending)) => {
                &pending.reference == reference && pending.selection == temp.selection
            }
            _ => false,
        };
        if state.revision != temp.revision || !matches {
            bail!("episode temporary selection changed");
        }
        journal.apply(
            phase(
                &p.source_id,
                &p.operation,
                &format!("handoff/withdraw/{}", temp.source),
            )?,
            if let Some(reference) = &temp.prepared {
                Command::Abort {
                    source,
                    owner: identity.owner.clone(),
                    expected_revision: temp.revision,
                    prepared: reference.clone(),
                }
            } else {
                Command::Withdraw {
                    source,
                    owner: identity.owner.clone(),
                    expected_revision: temp.revision,
                }
            },
        )?;
    }
    drop(guards);
    Ok(
        json!({"schema":HEADER,"selection":selection,"sources_released":temporary.len(),"sources_withdrawn":temporary.iter().filter(|temp|temp.prepared.is_none()).count(),"succeeded":succeeded,"temporary_owner_cleanup_required":false,
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

#[cfg(all(test, windows))]
mod recovery_tests;

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
