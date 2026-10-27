//! Declared consumed-input identity, separate from exact source provenance.
//! No producer execution, output approval, availability or removal authority.
use super::*;
use crate::native::BoundInstance;
use serde_json::json;

pub mod assessment;
pub mod produced;

/// Additional recipe lookup/graph work; traversal depth was checked by Graph.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub nodes: usize,
    pub edges: usize,
    pub bytes: usize,
}
impl Limits {
    fn validate(self) -> Result<()> {
        require(
            (1..=500_000).contains(&self.nodes)
                && (1..=2_000_000).contains(&self.edges)
                && (1..=268_435_456).contains(&self.bytes),
            "invalid recipe limits",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub name: String,
    pub semantics: Digest,
    pub producer: Object,
    /// Exact opaque request/profile parameter bytes, retained by the caller.
    /// An explicitly empty operation still supplies an explicit document pin.
    pub parameters: Object,
}

/// Externally admitted top-level properties. Empty means explicitly no authored
/// properties; qualified payloads and recursive production edges still count.
#[derive(Clone, Debug, Serialize)]
pub struct Consumption {
    pub definition_id: String,
    pub definition: Object,
    pub fields: BTreeSet<String>,
}

/// Explicit consumed subset of the graph's captured payloads, per definition.
/// Retention and graph provenance remain unchanged. Never inferred from a claim.
#[derive(Clone, Serialize)]
pub struct PayloadConsumption {
    pub definition_id: String,
    pub definition: Object,
    pub selected: bool,
    pub slots: BTreeSet<String>,
}
impl std::fmt::Debug for PayloadConsumption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PayloadConsumption")
            .field("selected", &self.selected)
            .field("slots", &self.slots.len())
            .finish()
    }
}

fn captures_selected(payloads: &Payloads) -> bool {
    matches!(
        payloads,
        Payloads::SelectedObject | Payloads::Named { selected: true, .. }
    )
}
fn captures_stream(payloads: &Payloads, slot: &str, role: &str, group: &str) -> bool {
    match payloads {
        Payloads::Streams { roles, groups, .. } => roles.contains(role) && groups.contains(group),
        Payloads::Named { slots, .. } => slots.contains(slot),
        _ => false,
    }
}
fn check_subset(
    bound: &BoundInstance,
    profile: &Profile,
    policy: &PayloadConsumption,
) -> Result<()> {
    let instance = bound.instance().value();
    require(
        !policy.selected || (captures_selected(&profile.payloads) && instance.selected.is_some()),
        "uncaptured selected payload",
    )?;
    for slot in &policy.slots {
        let stream = instance
            .streams
            .iter()
            .find(|s| &s.slot == slot)
            .ok_or_else(|| Error("missing consumed slot".into()))?;
        require(
            captures_stream(&profile.payloads, &stream.slot, &stream.role, &stream.group),
            "uncaptured consumed slot",
        )?;
    }
    Ok(())
}

/// Private exact provenance accompanies the declared consumed-input fingerprint.
/// Matching it is not proof of producer consumption or output correspondence.
#[derive(Debug)]
pub struct Plan {
    operation: Operation,
    fingerprint: Digest,
    checkpoint: Digest,
    binding: Digest,
    admission: Digest,
    consumption: Digest,
    nodes: BTreeMap<Reference, Digest>,
    bytes_read: usize,
}
impl Plan {
    pub fn operation(&self) -> &Operation {
        &self.operation
    }
    pub fn fingerprint(&self) -> &Digest {
        &self.fingerprint
    }
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn binding(&self) -> &Digest {
        &self.binding
    }
    pub fn admission(&self) -> &Digest {
        &self.admission
    }
    pub fn consumption(&self) -> &Digest {
        &self.consumption
    }
    pub fn nodes(&self) -> &BTreeMap<Reference, Digest> {
        &self.nodes
    }
    pub fn bytes_read(&self) -> usize {
        self.bytes_read
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
}

fn token(value: &str) -> Result<()> {
    require(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "invalid recipe token",
    )
}
fn hash(domain: &[u8]) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(b"TIG\0native-recipe-");
    hash.update(domain);
    hash.update(b"\0experimental-1\0");
    hash
}
fn done(hash: Sha256) -> Digest {
    Digest::from_sha256(hash.finalize().into())
}

fn read(
    source: &mut impl Source,
    graph: &Graph,
    reference: &Reference,
    remaining: usize,
) -> Result<(BoundInstance, usize)> {
    require(remaining > 0, "recipe byte budget")?;
    let lookup = source.lookup(reference, remaining);
    let (cost, bounded) = response_size(&lookup.data);
    require(bounded && cost <= remaining, "recipe byte budget")?;
    require(
        lookup.checkpoint == graph.checkpoint,
        "recipe checkpoint changed",
    )?;
    let Data::Record {
        instance,
        definition,
        schema,
    } = lookup.data
    else {
        return Err(Error("recipe source unavailable".into()));
    };
    let record = Instance::parse(&instance)?;
    require(
        record.object() == &reference.object && record.value().id == reference.id,
        "recipe record substitution",
    )?;
    let bound = record.bind(&definition, &schema)?;
    require(
        [&instance, &definition, &schema]
            .iter()
            .all(|b| graph.source_metadata.contains(&Object::from_bytes(b))),
        "recipe interpretation substitution",
    )?;
    Ok((bound, cost))
}

fn base(
    bound: &BoundInstance,
    profile: &Profile,
    consumption: &Consumption,
    projection: Option<&PayloadConsumption>,
) -> Result<Digest> {
    let instance = bound.instance().value();
    let definition = bound.definition().value();
    require(
        definition.id == consumption.definition_id,
        "recipe definition ID mismatch",
    )?;
    let mut hash = hash(b"base");
    feed(&mut hash, &(&instance.id, &definition.id, profile.class))?;
    hash.update((consumption.fields.len() as u64).to_be_bytes());
    for field in &consumption.fields {
        require(
            definition.authoring.properties.contains(field),
            "undeclared consumed property",
        )?;
        let value = instance
            .authored
            .get(field)
            .or_else(|| definition.authoring.defaults.get(field))
            .ok_or_else(|| Error("missing consumed property/default".into()))?;
        feed(&mut hash, &(field, value))?;
    }
    // Preserve payload roles and order, rather than hashing an unordered set.
    let selected = if let Some(policy) = projection {
        check_subset(bound, profile, policy)?;
        feed(
            &mut hash,
            &json!({"schema":"tig.native-consumed-payloads.experimental-1","selected":policy.selected,"slots":policy.slots}),
        )?;
        policy.selected
    } else {
        feed(&mut hash, &profile.payloads)?;
        captures_selected(&profile.payloads)
    };
    if selected {
        feed(&mut hash, &instance.selected)?;
    }
    let streams: Vec<_> = instance
        .streams
        .iter()
        .filter(|stream| {
            projection.map_or_else(
                || captures_stream(&profile.payloads, &stream.slot, &stream.role, &stream.group),
                |policy| policy.slots.contains(&stream.slot),
            )
        })
        .collect();
    hash.update((streams.len() as u64).to_be_bytes());
    for stream in streams {
        feed(
            &mut hash,
            &(&stream.slot, &stream.role, &stream.group, &stream.object),
        )?;
    }
    Ok(done(hash))
}

/// Compute a recipe for consuming an existing complete declared Production DAG.
/// Admission/eligibility and exact context protection remain separate gates.
pub fn plan(
    graph: &Graph,
    source: &mut impl Source,
    profiles: &[Profile],
    consumption: &[Consumption],
    operation: &Operation,
    limits: Limits,
) -> Result<Plan> {
    plan_inner(
        graph,
        source,
        profiles,
        consumption,
        operation,
        limits,
        None,
    )
}

/// Hash an independently admitted consumed payload subset while retaining the
/// original graph's complete captured requirements and provenance. Producers
/// remain responsible for declaring all consumed semantic fields/parameters.
pub fn plan_with_payloads(
    graph: &Graph,
    source: &mut impl Source,
    profiles: &[Profile],
    consumption: &[Consumption],
    payloads: &[PayloadConsumption],
    operation: &Operation,
    limits: Limits,
) -> Result<Plan> {
    plan_inner(
        graph,
        source,
        profiles,
        consumption,
        operation,
        limits,
        Some(payloads),
    )
}
fn plan_inner(
    graph: &Graph,
    source: &mut impl Source,
    profiles: &[Profile],
    consumption: &[Consumption],
    operation: &Operation,
    limits: Limits,
    payloads: Option<&[PayloadConsumption]>,
) -> Result<Plan> {
    let required = graph.production_requirements()?;
    limits.validate()?;
    token(&operation.name)?;
    for object in [&operation.producer, &operation.parameters] {
        require(
            object.bytes <= 9_007_199_254_740_991,
            "unsafe operation pin size",
        )?;
        require(
            graph
                .sizes
                .get(&object.sha256)
                .is_none_or(|bytes| *bytes == object.bytes),
            "operation and graph pin sizes conflict",
        )?;
    }
    require(
        operation.producer.sha256 != operation.parameters.sha256
            || operation.producer.bytes == operation.parameters.bytes,
        "operation pin sizes conflict",
    )?;
    require(
        graph.nodes.len() <= limits.nodes && graph.edges.len() <= limits.edges,
        "recipe graph budget",
    )?;
    require(
        profiles.len() <= 1024 && consumption.len() <= 1024,
        "recipe policy budget",
    )?;
    let mut admitted = BTreeMap::new();
    for profile in profiles {
        profile.validate()?;
        require(
            admitted.insert(&profile.definition, profile).is_none(),
            "duplicate recipe graph profile",
        )?;
    }
    let admission = canonical_json(
        &json!({"schema":"tig.native-profile-set.experimental-1","profiles":admitted.values().collect::<Vec<_>>()}),
    )?;
    require(
        admission == graph.admission_document,
        "recipe graph admission differs",
    )?;
    let mut fields = BTreeMap::new();
    for policy in consumption {
        Reference {
            id: policy.definition_id.clone(),
            object: policy.definition.clone(),
        }
        .validate()?;
        require(policy.fields.len() <= 256, "recipe property budget")?;
        for field in &policy.fields {
            token(field)?;
        }
        require(
            admitted
                .get(&policy.definition)
                .is_some_and(|p| p.definition_id == policy.definition_id),
            "unadmitted recipe definition",
        )?;
        require(
            fields.insert(&policy.definition, policy).is_none(),
            "duplicate consumption policy",
        )?;
    }
    let mut projected = BTreeMap::new();
    if let Some(policies) = payloads {
        require(
            policies.len() == admitted.len(),
            "complete payload policy required",
        )?;
        for policy in policies {
            Reference {
                id: policy.definition_id.clone(),
                object: policy.definition.clone(),
            }
            .validate()?;
            let profile = admitted
                .get(&policy.definition)
                .filter(|p| p.definition_id == policy.definition_id)
                .ok_or_else(|| Error("unadmitted payload definition".into()))?;
            require(policy.slots.len() <= 256, "consumed slot budget")?;
            require(
                !policy.selected || captures_selected(&profile.payloads),
                "uncaptured selected payload",
            )?;
            for slot in &policy.slots {
                require(
                    !slot.is_empty() && slot.len() <= 128 && !slot.chars().any(char::is_control),
                    "invalid consumed slot",
                )?;
                match &profile.payloads {
                    Payloads::Named { slots, .. } => {
                        require(slots.contains(slot), "uncaptured consumed slot")?
                    }
                    Payloads::Streams { .. } => {}
                    _ => return Err(Error("uncaptured consumed slot".into())),
                }
            }
            require(
                projected.insert(&policy.definition, policy).is_none(),
                "duplicate payload policy",
            )?;
        }
    }
    let consumption_document = if payloads.is_some() {
        json!({"schema":"tig.native-consumption.experimental-2","definitions":fields.values().collect::<Vec<_>>(),"payloads":projected.values().collect::<Vec<_>>(),"operation":operation})
    } else {
        json!({"schema":"tig.native-consumption.experimental-1","definitions":fields.values().collect::<Vec<_>>(),"operation":operation})
    };
    let consumption = Digest::blob(&canonical_json(&consumption_document)?);
    // All policy checks above precede extra source IO.
    let mut bases = BTreeMap::new();
    let mut bytes_read = 0;
    for (reference, node) in &graph.nodes {
        require(
            node.class != NodeKind::Output,
            "output cannot enter a production recipe",
        )?;
        let (bound, cost) = read(
            source,
            graph,
            reference,
            limits.bytes.saturating_sub(bytes_read),
        )?;
        bytes_read += cost;
        let definition = bound.definition().object();
        let profile = admitted
            .get(definition)
            .ok_or_else(|| Error("recipe definition missing".into()))?;
        let policy = fields
            .get(definition)
            .ok_or_else(|| Error("consumption policy missing".into()))?;
        require(
            payload_objects(bound.instance().value(), profile).as_ref()
                == graph
                    .declared_payloads
                    .get(reference)
                    .and_then(Option::as_ref),
            "recipe payload substitution",
        )?;
        let projection = if payloads.is_some() {
            Some(
                *projected
                    .get(definition)
                    .ok_or_else(|| Error("payload policy missing".into()))?,
            )
        } else {
            None
        };
        bases.insert(
            reference.clone(),
            base(&bound, profile, policy, projection)?,
        );
    }
    let mut outgoing: BTreeMap<&Reference, Vec<&Edge>> = BTreeMap::new();
    let mut parents: BTreeMap<&Reference, Vec<&Reference>> = BTreeMap::new();
    for edge in &graph.edges {
        require(
            edge.kind == EdgeKind::Production,
            "non-production recipe edge",
        )?;
        outgoing.entry(&edge.from).or_default().push(edge);
        parents.entry(&edge.target).or_default().push(&edge.from);
    }
    let mut pending: BTreeMap<_, _> = graph
        .nodes
        .keys()
        .map(|r| (r, outgoing.get(r).map_or(0, Vec::len)))
        .collect();
    let mut ready: VecDeque<_> = pending
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(r, _)| *r)
        .collect();
    let mut nodes = BTreeMap::new();
    while let Some(reference) = ready.pop_front() {
        let mut hash = hash(b"node");
        feed(&mut hash, &bases[reference])?;
        let children = outgoing.get(reference).map_or(&[][..], Vec::as_slice);
        hash.update((children.len() as u64).to_be_bytes());
        for edge in children {
            feed(&mut hash, &(&edge.pointer, &nodes[&edge.target]))?;
        }
        nodes.insert(reference.clone(), done(hash));
        if let Some(parents) = parents.get(reference) {
            for parent in parents {
                let n = pending
                    .get_mut(parent)
                    .ok_or_else(|| Error("recipe parent missing".into()))?;
                *n = n
                    .checked_sub(1)
                    .ok_or_else(|| Error("recipe degree invalid".into()))?;
                if *n == 0 {
                    ready.push_back(*parent);
                }
            }
        }
    }
    require(
        nodes.len() == graph.nodes.len(),
        "recipe cycle or incomplete DAG",
    )?;
    let mut roots = graph
        .roots
        .iter()
        .map(|r| nodes[r].clone())
        .collect::<Vec<_>>();
    roots.sort();
    let mut fingerprint = hash(b"root");
    feed(&mut fingerprint, operation)?;
    fingerprint.update((roots.len() as u64).to_be_bytes());
    for root in roots {
        feed(&mut fingerprint, &root)?;
    }
    Ok(Plan {
        operation: operation.clone(),
        fingerprint: done(fingerprint),
        checkpoint: graph.checkpoint.clone(),
        binding: required.binding_digest,
        admission: graph.admission.clone(),
        consumption,
        nodes,
        bytes_read,
    })
}
