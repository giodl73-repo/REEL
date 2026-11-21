//! Immutable terminal outcomes in the existing tree/store, selected before old
//! versions withdraw. Unknown archives never mean a previously unseen request.
use super::*;
use std::collections::BTreeMap;

const HISTORY: &str = "tig.protection-effect-history.design-1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    index: Digest,
    object: Object,
    tree: Object,
}
pub(super) struct ReadOnly<'a, S: ?Sized>(pub &'a S);
impl<S: NodeStore + ?Sized> NodeStore for ReadOnly<'_, S> {
    fn get(&self, object: &Object) -> std::result::Result<Option<Vec<u8>>, TreeError> {
        self.0.get(object)
    }
    fn put(&mut self, _: &[u8]) -> std::result::Result<Object, TreeError> {
        Err(TreeError::InvalidNode)
    }
}
pub(super) fn limits() -> TreeLimits {
    TreeLimits {
        max_entries: 4096,
        max_reads: 8192,
        max_read_bytes: 8 * 1024 * 1024,
        max_writes: 8192,
        max_write_bytes: 8 * 1024 * 1024,
        max_map_depth: 64,
    }
}
fn context() -> Digest {
    Digest::blob(HISTORY.as_bytes())
}
pub(super) fn is_archive(selection: &Selection) -> bool {
    selection.context == context()
}
fn path(request: &Request) -> Result<LogicalPath> {
    LogicalPath::parse(&format!(
        "{}-{}",
        request.store.as_str(),
        Digest::blob(request.operation.as_str().as_bytes()).as_str()
    ))
    .map_err(|_| Error::Invalid)
}
fn header(
    index: &Index,
    nodes: &impl NodeStore,
    selection: &Selection,
    object: &Object,
) -> Result<Header> {
    if !is_archive(selection) || selection.root.bytes > 4096 {
        return Err(Error::Invalid);
    }
    let header: Header = load(nodes, &selection.root)?;
    if header.schema != HISTORY || header.index != *index.identity() || header.object != *object {
        return Err(Error::Invalid);
    }
    Ok(header)
}
fn terminal(
    index: &Index,
    nodes: &impl NodeStore,
    root: &Object,
    object: &Object,
) -> Result<Record> {
    let record = read(
        index,
        nodes,
        &Selection {
            root: root.clone(),
            context: marker(true),
        },
    )?;
    if record.request.object != *object || record.outcome.is_none() {
        return Err(Error::Invalid);
    }
    Ok(record)
}
pub(super) fn lookup(
    index: &Index,
    nodes: &mut impl NodeStore,
    selection: &Selection,
    request: &Request,
) -> Result<Option<Record>> {
    let header = header(index, nodes, selection, &request.object)?;
    let (root, _) = Tree::from_root(header.tree).get(&path(request)?, nodes, limits())?;
    let Some(root) = root else {
        return Ok(None);
    };
    let record = terminal(index, nodes, &root, &request.object)?;
    if record.request != *request {
        return Err(Error::Conflict);
    }
    Ok(Some(record))
}
fn settled(index: &mut Index, record: &Record) -> Result<()> {
    let id = budget_id(index.identity(), &record.request)?;
    let Some((reference, closed, spent)) = progress_budget::reservation(&mut index.journal, &id)?
    else {
        return Err(Error::Unknown);
    };
    if reference != record.budget || !closed || spent != 4 {
        return Err(Error::Pending);
    }
    Ok(())
}
pub(super) fn compact(
    index: &mut Index,
    nodes: &mut metadata::MetadataStore,
    object: &Object,
    state: &Source,
    authority: u64,
) -> Result<usize> {
    compact_hook(index, nodes, object, state, authority, || Ok(()))
}
pub(super) fn compact_hook(
    index: &mut Index,
    nodes: &mut metadata::MetadataStore,
    object: &Object,
    state: &Source,
    authority: u64,
    mut selected: impl FnMut() -> Result<()>,
) -> Result<usize> {
    if state.owner != index.enrollment.owner || state.cancelled || state.pending.is_some() {
        return Err(Error::Pending);
    }
    let current = state.current.as_ref().ok_or(Error::Unknown)?;
    if current.selection.context != marker(true) {
        return Err(Error::Pending);
    }
    let mut work = Work::bounded(nodes, limits());
    let latest = terminal(index, &work, &current.selection.root, object)?;
    settled(index, &latest)?;
    if state.predecessors.is_empty() {
        return Ok(0);
    }
    let mut archived = None;
    let mut records = BTreeMap::new();
    let mut pending = Vec::new();
    for version in state.predecessors.iter().chain(state.current.iter()) {
        if is_archive(&version.selection) {
            if archived.is_some() {
                return Err(Error::Invalid);
            }
            let h = header(index, &work, &version.selection, object)?;
            archived = Some((version.clone(), h));
        } else {
            let record = read(index, &work, &version.selection)?;
            if record.request.object != *object {
                return Err(Error::Invalid);
            }
            if record.outcome.is_some() {
                settled(index, &record)?;
                let key = path(&record.request)?;
                if records
                    .insert(key, (version.selection.root.clone(), record))
                    .is_some()
                {
                    return Err(Error::Conflict);
                }
            } else {
                pending.push(record);
            }
        }
    }
    let tree = match &archived {
        Some((_, h)) => Tree::from_root(h.tree.clone()),
        None => Tree::empty(&mut work)?,
    };
    let mut changes = BTreeMap::new();
    for (key, (root, _)) in &records {
        let (old, _) = tree.get(key, &mut work, limits())?;
        if let Some(old) = old {
            if old != *root {
                return Err(Error::Conflict);
            }
        } else {
            changes.insert(key.clone(), Some(root.clone()));
        }
    }
    for record in pending {
        let terminal = if let Some((_, value)) = records.get(&path(&record.request)?) {
            value.clone()
        } else if let Some((version, _)) = &archived {
            lookup(index, &mut work, &version.selection, &record.request)?.ok_or(Error::Unknown)?
        } else {
            return Err(Error::Unknown);
        };
        let mut matching = terminal;
        matching.outcome = None;
        if matching != record {
            return Err(Error::Conflict);
        }
    }
    let (tree, _) = tree.edit(tree.root(), &changes, &mut work, limits())?;
    let root = put(
        &mut work,
        &Header {
            schema: HISTORY.into(),
            index: index.identity().clone(),
            object: object.clone(),
            tree: tree.root().clone(),
        },
    )?;
    work.inner.finish()?;
    let selection = Selection {
        root,
        context: context(),
    };
    header(index, &work, &selection, object)?;
    for (_, record) in records.values() {
        let found = lookup(index, &mut work, &selection, &record.request)?.ok_or(Error::Unknown)?;
        if found != *record {
            return Err(Error::Conflict);
        }
    }
    let anchor = archived
        .as_ref()
        .map_or(&state.predecessors[0], |(version, _)| version);
    let mut revision = state.revision;
    let mut retired = 0;
    if anchor.selection != selection {
        replace(
            index,
            object,
            revision,
            anchor.revision,
            Some(selection),
            authority,
        )?;
        revision = revision.checked_add(1).ok_or(Error::Budget)?;
        retired += 1;
    }
    // The new archive is durable and selected before any redundant references
    // withdraw. Recovery can resume here without losing any historical outcome.
    selected()?;
    for version in &state.predecessors {
        if version.revision == anchor.revision {
            continue;
        }
        replace(index, object, revision, version.revision, None, authority)?;
        revision = revision.checked_add(1).ok_or(Error::Budget)?;
        retired += 1;
    }
    Ok(retired)
}
fn replace(
    index: &mut Index,
    object: &Object,
    revision: u64,
    previous: u64,
    replacement: Option<Selection>,
    authority: u64,
) -> Result<()> {
    let operation = Key::parse(&format!(
        "effect-history-{}",
        Digest::blob(&encode(&(
            HISTORY,
            index.identity(),
            object,
            revision,
            previous,
            &replacement
        ))?)
        .as_str()
    ))
    .map_err(|_| Error::Invalid)?;
    index.command(
        operation,
        Command::RetireGuarded {
            source: slot(object)?,
            owner: index.enrollment.owner.clone(),
            expected_revision: revision,
            predecessor_revision: previous,
            replacement,
            guards: vec![Guard {
                source: key("authority"),
                owner: index.enrollment.owner.clone(),
                expected_revision: authority,
            }],
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod vectors {
    use super::*;
    #[test]
    fn archive_codec_has_stable_canonical_vector_and_legacy_record_rejects_it() {
        const BODY: &[u8] = br#"{"index":"0000000000000000000000000000000000000000000000000000000000000000","object":{"bytes":1,"sha256":"1111111111111111111111111111111111111111111111111111111111111111"},"schema":"tig.protection-effect-history.design-1","tree":{"bytes":2,"sha256":"2222222222222222222222222222222222222222222222222222222222222222"}}"#;
        let value: Header = serde_json::from_slice(BODY).unwrap();
        assert_eq!(encode(&value).unwrap(), BODY);
        assert_eq!(
            context().as_str(),
            "98cf176fcc1c0183bddecb53671c08be341add16a225317d1891cad9a25f53e4"
        );
        assert_eq!(
            Digest::blob(BODY).as_str(),
            "def3f53cbff5fcd3d02a1d01a707f123e5f12425c9998225b359953ff62119d6"
        );
        assert!(serde_json::from_slice::<Record>(BODY).is_err());
        let mut extra: serde_json::Value = serde_json::from_slice(BODY).unwrap();
        extra["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Header>(extra).is_err());
    }
}
