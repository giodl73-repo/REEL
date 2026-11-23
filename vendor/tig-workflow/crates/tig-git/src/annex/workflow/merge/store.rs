use super::*;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: String,
    body: Object,
    budget: Option<BudgetRef>,
    guarded: bool,
}
pub(super) fn put(nodes: &mut MetadataStore, value: &impl Serialize) -> io::Result<Object> {
    let bytes = encode(value)?;
    if bytes.len() > 65536 {
        return Err(invalid());
    }
    nodes.put(&bytes).map_err(|_| invalid())
}
pub(super) fn bytes(nodes: &impl NodeStore, root: &Object) -> io::Result<Vec<u8>> {
    if root.bytes == 0 || root.bytes > 65536 {
        return Err(invalid());
    }
    let b = nodes
        .get(root)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if Object::from_bytes(&b) != *root {
        return Err(invalid());
    }
    Ok(b)
}
pub(super) fn read<T: serde::de::DeserializeOwned>(
    nodes: &impl NodeStore,
    root: &Object,
) -> io::Result<T> {
    let b = bytes(nodes, root)?;
    let value = tig_core::identity::parse_json(&b).map_err(|_| invalid())?;
    if tig_core::identity::canonical_json(&value).map_err(|_| invalid())? != b {
        return Err(invalid());
    }
    serde_json::from_value(value).map_err(|_| invalid())
}
pub(super) fn version<T: serde::de::DeserializeOwned>(
    nodes: &impl NodeStore,
    version: &source_claims::Version,
) -> io::Result<T> {
    if version.selection.context != Digest::blob(SCHEMA.as_bytes()) {
        return Err(invalid());
    }
    let envelope: Envelope = read(nodes, &version.selection.root)?;
    if envelope.schema != SCHEMA {
        return Err(invalid());
    }
    read(nodes, &envelope.body)
}
fn adoption(
    j: &mut SourceJournal,
    source: &Key,
    owner: &Key,
    revision: u64,
    prepared: source_claims::Reference,
    guarded: bool,
) -> io::Result<Command> {
    if guarded {
        let cfg = j
            .source(&k("configuration")?)
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        Ok(Command::AdoptGuarded {
            source: source.clone(),
            owner: owner.clone(),
            expected_revision: revision,
            prepared,
            guards: vec![source_claims::Guard {
                source: k("configuration")?,
                owner: owner.clone(),
                expected_revision: cfg.revision,
            }],
        })
    } else {
        Ok(Command::Adopt {
            source: source.clone(),
            owner: owner.clone(),
            expected_revision: revision,
            prepared,
        })
    }
}
fn adopt_key(reference: &source_claims::Reference) -> io::Result<Key> {
    key("adopt", reference)
}
fn apply(
    j: &mut SourceJournal,
    op: Key,
    command: Command,
    budget: Option<&BudgetRef>,
) -> io::Result<source_claims::Receipt> {
    j.apply(
        op,
        match budget {
            Some(b) => b.wrap(command),
            None => command,
        },
    )
    .map_err(|_| invalid())
}
pub(super) fn load<T: serde::de::DeserializeOwned>(
    j: &mut SourceJournal,
    nodes: &MetadataStore,
    source: &Key,
    owner: &Key,
) -> io::Result<Option<T>> {
    let Some(mut state) = j.source(source).map_err(|_| invalid())? else {
        return Ok(None);
    };
    if state.owner != *owner || state.cancelled {
        return Err(invalid());
    }
    if let Some(p) = state.pending {
        let e: Envelope = read(nodes, &p.selection.root)?;
        if e.schema != SCHEMA {
            return Err(invalid());
        }
        let op = adopt_key(&p.reference)?;
        let command = adoption(j, source, owner, state.revision, p.reference, e.guarded)?;
        apply(j, op, command, e.budget.as_ref())?;
        state = j
            .source(source)
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
    }
    let e: Envelope = read(nodes, &state.current.ok_or_else(invalid)?.selection.root)?;
    if e.schema != SCHEMA {
        return Err(invalid());
    }
    read(nodes, &e.body).map(Some)
}
pub(super) fn save(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    source: &Key,
    owner: &Key,
    value: &impl Serialize,
    budget: Option<&BudgetRef>,
    guarded: bool,
) -> io::Result<()> {
    let body = put(nodes, value)?;
    if body.bytes
        > if source.as_str() == "configuration" {
            65536
        } else {
            4096
        }
    {
        return Err(invalid());
    }
    let root = put(
        nodes,
        &Envelope {
            schema: SCHEMA.into(),
            body,
            budget: budget.cloned(),
            guarded,
        },
    )?;
    nodes.finish().map_err(|_| invalid())?;
    let state = j.source(source).map_err(|_| invalid())?;
    if state.as_ref().is_some_and(|s| s.pending.is_some()) {
        return Err(invalid());
    }
    if state
        .as_ref()
        .and_then(|s| s.current.as_ref())
        .is_some_and(|v| v.selection.root == root)
    {
        return Ok(());
    }
    let revision = state.map_or(0, |s| s.revision);
    let receipt = apply(
        j,
        key("prepare", &(j.identity(), source, revision, &root))?,
        Command::Prepare {
            source: source.clone(),
            owner: owner.clone(),
            expected_revision: revision,
            selection: Selection {
                root,
                context: Digest::blob(SCHEMA.as_bytes()),
            },
        },
        budget,
    )?;
    let op = adopt_key(&receipt.reference)?;
    let command = adoption(j, source, owner, revision + 1, receipt.reference, guarded)?;
    apply(j, op, command, budget)?;
    Ok(())
}
pub(super) fn reserve(
    j: &mut SourceJournal,
    owner: &Key,
    source: &Key,
    operation: &Digest,
    role: &str,
) -> io::Result<BudgetRef> {
    let first = j
        .source(source)
        .map_err(|_| invalid())?
        .map_or(0, |s| s.revision);
    let guard = j
        .source(&k("configuration")?)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    progress_budget::reserve(
        j,
        key(role, operation)?,
        owner.clone(),
        source.clone(),
        first,
        vec![source_claims::Guard {
            source: k("configuration")?,
            owner: owner.clone(),
            expected_revision: guard.revision,
        }],
    )
    .map_err(|_| invalid())
}
pub(super) struct Session {
    pub journal: SourceJournal,
    pub nodes: MetadataStore,
    pub enrolled: Enrolled,
    pub handle: Handle,
    pub head: Head,
}
impl Session {
    pub fn open(handle: &Handle) -> io::Result<Self> {
        let mut journal =
            SourceJournal::open(&handle.root, &handle.minimum).map_err(|_| invalid())?;
        if journal.identity() != &handle.journal {
            return Err(invalid());
        }
        let nodes = MetadataStore::open(&handle.metadata.root, &handle.metadata.minimum)
            .map_err(|_| invalid())?;
        let enrolled = enrollment(handle, &mut journal, &nodes)?;
        let head: Head =
            load(&mut journal, &nodes, &k("head")?, &enrolled.owner)?.ok_or_else(invalid)?;
        stronger(&head.index_minimum, &handle.index_minimum)?;
        stronger(&head.policy_minimum, &handle.policy_minimum)?;
        Ok(Self {
            journal,
            nodes,
            enrolled,
            handle: handle.clone(),
            head,
        })
    }
    pub fn handle(&self) -> Handle {
        let mut h = self.handle.clone();
        h.minimum = self.journal.checkpoint();
        h.metadata.minimum = self.nodes.checkpoint();
        h.index_minimum = stronger(&self.head.index_minimum, &self.handle.index_minimum)
            .unwrap_or_else(|_| self.handle.index_minimum.clone());
        h.policy_minimum = stronger(&self.head.policy_minimum, &self.handle.policy_minimum)
            .unwrap_or_else(|_| self.handle.policy_minimum.clone());
        h
    }
    pub fn config(&self) -> io::Result<Config> {
        let mut c = self.enrolled.config.clone();
        c.package.index_minimum = stronger(&self.head.index_minimum, &self.handle.index_minimum)?;
        let mut latest = c.policy.minimum.clone();
        for min in [
            &self.enrolled.config.policy.minimum,
            &self.head.policy_minimum,
            &self.handle.policy_minimum,
        ] {
            let r = tig_store::source_claims::policy_registry::Registry::open(&c.policy.root, min)
                .map_err(|_| invalid())?;
            if r.identity() != &c.policy.registry {
                return Err(invalid());
            }
            latest = r.checkpoint();
        }
        c.policy.minimum = latest;
        Ok(c)
    }
    pub fn index(&self) -> io::Result<Index> {
        let c = &self.enrolled.config;
        let mut latest = None;
        for min in [
            &c.package.index_minimum,
            &self.head.index_minimum,
            &self.handle.index_minimum,
        ] {
            let index = Index::open(&c.package.index_root, min).map_err(|_| invalid())?;
            if index.identity() != &c.package.index || !index.supports_effect_fences() {
                return Err(invalid());
            }
            latest = Some(index);
        }
        latest.ok_or_else(invalid)
    }
    pub fn head(&mut self, budget: Option<&BudgetRef>) -> io::Result<()> {
        save(
            &mut self.journal,
            &mut self.nodes,
            &k("head")?,
            &self.enrolled.owner,
            &self.head,
            budget,
            budget.is_some() && self.head.settled,
        )
    }
    pub fn saved(&mut self, op: &Digest) -> io::Result<Option<Saved>> {
        load(
            &mut self.journal,
            &self.nodes,
            &run_key(op)?,
            &self.enrolled.owner,
        )
    }
    pub fn save(&mut self, op: &Digest, state: &Saved) -> io::Result<()> {
        let first = self
            .journal
            .source(&run_key(op)?)
            .map_err(|_| invalid())?
            .is_none();
        save(
            &mut self.journal,
            &mut self.nodes,
            &run_key(op)?,
            &self.enrolled.owner,
            state,
            Some(&state.run_budget),
            first,
        )
    }
}

pub(super) fn enrollment(
    handle: &Handle,
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
) -> io::Result<Enrolled> {
    if journal.identity() != &handle.journal {
        return Err(invalid());
    }
    // Enrollment is an acknowledged immutable ordinary journal source.
    let source = journal
        .source(&k("configuration")?)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if source.pending.is_some() || source.cancelled || source.revision != 2 {
        return Err(invalid());
    }
    let e: Envelope = read(nodes, &source.current.ok_or_else(invalid)?.selection.root)?;
    if e.schema != SCHEMA || e.body != handle.configuration {
        return Err(invalid());
    }
    let enrolled: Enrolled = read(nodes, &e.body)?;
    if enrolled.schema != SCHEMA
        || enrolled.owner != source.owner
        || enrolled.config.package.metadata.root != handle.metadata.root
    {
        return Err(invalid());
    }
    Ok(enrolled)
}
/// Read only an acknowledged source. Unlike load(), this never adopts pending
/// metadata, reserves progress or issues any journal command.
pub(super) fn selected<T: serde::de::DeserializeOwned>(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    source: &Key,
    owner: &Key,
) -> io::Result<Option<T>> {
    let Some(state) = journal.source(source).map_err(|_| invalid())? else {
        return Ok(None);
    };
    if state.owner != *owner || state.cancelled || state.pending.is_some() {
        return Err(invalid());
    }
    let e: Envelope = read(nodes, &state.current.ok_or_else(invalid)?.selection.root)?;
    if e.schema != SCHEMA {
        return Err(invalid());
    }
    read(nodes, &e.body).map(Some)
}
