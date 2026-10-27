//! Proposals from an exact archived record; never current selection or approval.
use super::*;
use crate::tig_scene_production::{OutputBinding, RenderedCandidate};
use tig_assets::graph::recipe::produced::{BoundSubject, Selector};
use tig_assets::{
    graph::{self, Reference},
    native::{Instance, Pin, Stream},
    resolve::AssetResolver,
};
use tig_git::source::native::{AssetSnapshot, context::ProductionContext};

const MAX_RECORDS: usize = 128;
const BYTES: u64 = 65_536;
const CLAIM: &str = "production-claim.json";
const INDEX: &str = "record-candidates.json";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Spec {
    pub output: String,
    pub template: Reference,
    pub selector: Selector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_class: Option<StreamClass>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StreamClass {
    role: String,
    group: String,
}
struct Template {
    reference: Reference,
    instance: Instance,
    definition: Vec<u8>,
    schema: Vec<u8>,
}
pub(super) struct Prepared {
    specs: Vec<Spec>,
    templates: BTreeMap<String, Template>,
    handoff: Option<handoff::Handoff>,
    roots: Vec<PathBuf>,
}
fn path(id: &str) -> String {
    format!("records/{}.tig", Digest::blob(id.as_bytes()).as_str())
}
pub(super) fn artifacts(worker: &Worker) -> Result<BTreeMap<String, u64>> {
    validate_specs(&worker.record_outputs)?;
    if let Some(handoff) = &worker.git_handoff {
        handoff.validate(&worker.record_outputs)?;
    }
    if worker.record_outputs.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut result = BTreeMap::from([(CLAIM.into(), BYTES), (INDEX.into(), BYTES)]);
    if worker.git_handoff.is_some() {
        result.insert(handoff::FILE.into(), BYTES);
    }
    for spec in &worker.record_outputs {
        result.insert(path(&spec.template.id), BYTES);
    }
    Ok(result)
}
fn validate_specs(specs: &[Spec]) -> Result<()> {
    if specs.len() > MAX_RECORDS {
        bail!("native record mapping exceeds budget");
    }
    let mut slots = BTreeSet::new();
    let mut references = BTreeMap::new();
    let token = |s: &str| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control);
    for spec in specs {
        LogicalPath::parse(&spec.output)?;
        if spec.output == "receipt.json"
            || !slots.insert((spec.template.id.clone(), spec.selector.clone()))
        {
            bail!("duplicate or unsupported native record selector");
        }
        if references
            .insert(&spec.template.id, &spec.template.object)
            .is_some_and(|old| old != &spec.template.object)
        {
            bail!("conflicting native record templates");
        }
        match (&spec.selector, &spec.stream_class) {
            (Selector::Selected {}, None) => (),
            (Selector::Stream { slot }, Some(class))
                if token(slot) && token(&class.role) && token(&class.group) => {}
            _ => bail!("native record selector requires compatible explicit stream class"),
        }
    }
    Ok(())
}
pub(super) fn check_names(worker: &Worker, names: &BTreeSet<String>) -> Result<()> {
    artifacts(worker)?;
    if worker
        .record_outputs
        .iter()
        .any(|s| s.output == "receipt.json" || !names.contains(&s.output))
    {
        bail!("native record mapping names an undeclared media output");
    }
    Ok(())
}
fn payload(object: &Object) -> Pin {
    Pin {
        id: None,
        sha256: object.sha256.clone(),
        bytes: object.bytes,
        media_type: None,
    }
}
fn assign(instance: &mut Instance, spec: &Spec, object: &Object) -> Result<()> {
    match &spec.selector {
        Selector::Selected {} => {
            if spec.stream_class.is_some() {
                bail!("selected output cannot specify a stream class");
            }
            instance.selected = Some(payload(object));
        }
        Selector::Stream { slot } => {
            let class = spec
                .stream_class
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("named output requires explicit stream class"))?;
            if let Some(stream) = instance.streams.iter_mut().find(|s| s.slot == *slot) {
                if stream.role != class.role || stream.group != class.group {
                    bail!("native output stream class differs from template");
                }
                stream.object = payload(object);
                stream.provenance = None;
            } else {
                instance.streams.push(Stream {
                    slot: slot.clone(),
                    role: class.role.clone(),
                    group: class.group.clone(),
                    object: payload(object),
                    provenance: None,
                });
            }
        }
    }
    Ok(())
}
fn bind(value: &Instance, template: &Template) -> Result<tig_assets::native::BoundInstance> {
    let bytes = identity::canonical_json(&serde_json::to_value(value)?)?;
    if bytes.len() > BYTES as usize {
        bail!("native candidate record exceeds budget");
    }
    Ok(Instance::parse(&bytes)?.bind(&template.definition, &template.schema)?)
}
pub(super) fn prepare(
    worker: &Worker,
    context: &ProductionContext,
    roots: &[PathBuf],
) -> Result<Prepared> {
    artifacts(worker)?;
    if worker.record_outputs.is_empty() {
        return Ok(Prepared {
            specs: vec![],
            templates: BTreeMap::new(),
            handoff: None,
            roots: vec![],
        });
    }
    let profile: ArchiveProfile = serde_json::from_value(worker.source.clone())?;
    if &profile.archive != context.archive()
        || &profile.checkpoint != context.requirements().checkpoint()
    {
        bail!("record templates belong to another native context");
    }
    let mut reader = MetadataReader::open(&worker.metadata_root, &worker.metadata_minimum)?;
    let mut snapshot = AssetSnapshot::from_metadata_archive(
        &mut reader,
        &profile.archive,
        profile.limits,
        TreeLimits::default(),
    )?;
    if snapshot.checkpoint() != &profile.checkpoint {
        bail!("record template checkpoint differs");
    }
    let mut prepared = Prepared {
        specs: worker.record_outputs.clone(),
        templates: BTreeMap::new(),
        handoff: worker.git_handoff.clone(),
        roots: if worker.git_handoff.is_some() {
            roots.to_vec()
        } else {
            vec![]
        },
    };
    let mut slots = BTreeSet::new();
    for spec in &worker.record_outputs {
        LogicalPath::parse(&spec.output)?;
        if !slots.insert((spec.template.id.clone(), spec.selector.clone())) {
            bail!("duplicate native record selector");
        }
        if context
            .requirements()
            .metadata()
            .contains(&spec.template.object)
        {
            bail!("native output cannot overwrite its production input record");
        }
        if let Some(old) = prepared.templates.get(&spec.template.id) {
            if old.reference != spec.template {
                bail!("conflicting native record templates");
            }
            continue;
        }
        let resolved = snapshot.resolve_id(&spec.template.id)?;
        if resolved.reference() != &spec.template
            || !profile.admitted.iter().any(|a| {
                a.id == resolved.definition().id && a.object == resolved.definition().object
            })
        {
            bail!("native record template or definition is not admitted");
        }
        let lookup = graph::Source::lookup(&mut snapshot, &spec.template, 3 * BYTES as usize);
        if lookup.checkpoint != profile.checkpoint {
            bail!("native record template view changed");
        }
        let graph::Data::Record {
            instance,
            definition,
            schema,
        } = lookup.data
        else {
            bail!("native record template metadata unavailable");
        };
        if Object::from_bytes(&instance) != spec.template.object {
            bail!("native record template bytes differ");
        }
        let bound = Instance::parse(&instance)?.bind(&definition, &schema)?;
        prepared.templates.insert(
            spec.template.id.clone(),
            Template {
                reference: spec.template.clone(),
                instance: bound.instance().value().clone(),
                definition,
                schema,
            },
        );
    }
    let mut shapes: BTreeMap<_, _> = prepared
        .templates
        .iter()
        .map(|(id, t)| (id.clone(), t.instance.clone()))
        .collect();
    for spec in &prepared.specs {
        let shape = shapes.get_mut(&spec.template.id).unwrap();
        assign(
            shape,
            spec,
            &Object {
                sha256: Digest::blob(b"native record preflight placeholder"),
                bytes: 9_007_199_254_740_991,
            },
        )?;
    }
    let claim_placeholder = Object {
        sha256: Digest::blob(b"native claim preflight"),
        bytes: BYTES,
    };
    let receipt_placeholder = Object {
        sha256: Digest::blob(b"native receipt preflight"),
        bytes: crate::tig_inputs::JOB_BYTES_MAX as u64,
    };
    let mut record_shapes = vec![];
    for (id, shape) in &mut shapes {
        let template = &prepared.templates[id];
        annotate(
            shape,
            template,
            &prepared.specs,
            &claim_placeholder,
            &receipt_placeholder,
        )?;
        bind(shape, template)?;
        record_shapes.push(CandidateRecord {
            asset_id: id.clone(),
            template: template.reference.clone(),
            path: path(id),
            object: claim_placeholder.clone(),
        });
    }
    let handoff_shape = prepared
        .handoff
        .as_ref()
        .map(|h| {
            h.profile(
                record_shapes
                    .iter()
                    .map(|r| (r.asset_id.as_str(), &r.template.object, &r.object)),
                &prepared.roots,
            )
        })
        .transpose()?;
    let index_shape = CandidateSet {
        schema: "reel.native-record-candidate-set.experimental-1".into(),
        claim: claim_placeholder,
        receipt: receipt_placeholder,
        records: record_shapes,
        git_proposal: handoff_shape.as_deref().map(Object::from_bytes),
    };
    if identity::canonical_json(&serde_json::to_value(index_shape)?)?.len() > BYTES as usize {
        bail!("native candidate index exceeds preflight budget");
    }
    Ok(prepared)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateRecord {
    asset_id: String,
    template: Reference,
    path: String,
    object: Object,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateSet {
    schema: String,
    claim: Object,
    receipt: Object,
    records: Vec<CandidateRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git_proposal: Option<Object>,
}
fn write(name: &str, bytes: &[u8]) -> Result<()> {
    if bytes.len() > BYTES as usize {
        bail!("native record artifact exceeds budget");
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(name)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn annotate(
    value: &mut Instance,
    template: &Template,
    specs: &[Spec],
    claim: &Object,
    receipt: &Object,
) -> Result<()> {
    value.status.clear();
    value.metadata.clear();
    let selectors: Vec<_> = specs
        .iter()
        .filter(|s| s.template.id == value.id)
        .map(|s| s.selector.clone())
        .collect();
    value.metadata.insert("production".into(),json!({"schema":"reel.native-record-candidate.experimental-1","template":template.reference,"claim":claim,"receipt":receipt,"selectors":selectors}));
    for spec in specs.iter().filter(|s| s.template.id == value.id) {
        if let Selector::Stream { slot } = &spec.selector {
            value
                .streams
                .iter_mut()
                .find(|s| s.slot == *slot)
                .ok_or_else(|| anyhow::anyhow!("candidate stream missing"))?
                .provenance = Some(serde_json::from_value(
                json!({"claim":claim,"receipt":receipt}),
            )?);
        }
    }
    Ok(())
}
pub(super) fn emit(prepared: &Prepared, candidate: &RenderedCandidate) -> Result<Option<Value>> {
    if prepared.specs.is_empty() {
        return Ok(None);
    }
    let mut values: BTreeMap<_, _> = prepared
        .templates
        .iter()
        .map(|(id, t)| {
            let mut value = t.instance.clone();
            value.status.clear();
            value.metadata.clear();
            (id.clone(), value)
        })
        .collect();
    for spec in &prepared.specs {
        let item = candidate
            .receipt()
            .outputs
            .get(&spec.output)
            .ok_or_else(|| anyhow::anyhow!("native record output not produced"))?;
        assign(
            values.get_mut(&spec.template.id).unwrap(),
            spec,
            &Object {
                sha256: Digest::parse(&item.sha256)?,
                bytes: item.bytes,
            },
        )?;
    }
    let subjects = prepared
        .specs
        .iter()
        .map(|spec| -> Result<_> {
            let template = &prepared.templates[&spec.template.id];
            let bound = bind(&values[&spec.template.id], template)?;
            let definition = Reference {
                id: bound.definition().value().id.clone(),
                object: bound.definition().object().clone(),
            };
            let subject = BoundSubject::bind(&bound, &definition, spec.selector.clone())?
                .ok_or_else(|| anyhow::anyhow!("native candidate subject missing"))?;
            Ok((spec.output.clone(), subject))
        })
        .collect::<Result<Vec<_>>>()?;
    let bindings: Vec<_> = subjects
        .iter()
        .map(|(name, subject)| OutputBinding {
            output_name: name,
            subject,
        })
        .collect();
    let claim = candidate.claim(&bindings)?;
    let claim = tig_assets::graph::recipe::produced::Claim::parse(claim.object(), claim.bytes())?;
    let mut records = vec![];
    let mut bytes_by_path = BTreeMap::new();
    for (id, mut value) in values {
        let template = &prepared.templates[&id];
        annotate(
            &mut value,
            template,
            &prepared.specs,
            claim.object(),
            candidate.receipt_object(),
        )?;
        bind(&value, template)?;
        let bytes = identity::canonical_json(&serde_json::to_value(&value)?)?;
        let name = path(&id);
        records.push(CandidateRecord {
            asset_id: id,
            template: template.reference.clone(),
            path: name.clone(),
            object: Object::from_bytes(&bytes),
        });
        bytes_by_path.insert(name, bytes);
    }
    let handoff_bytes = prepared
        .handoff
        .as_ref()
        .map(|h| {
            h.profile(
                records
                    .iter()
                    .map(|r| (r.asset_id.as_str(), &r.template.object, &r.object)),
                &prepared.roots,
            )
        })
        .transpose()?;
    let set = CandidateSet {
        schema: "reel.native-record-candidate-set.experimental-1".into(),
        claim: claim.object().clone(),
        receipt: candidate.receipt_object().clone(),
        records,
        git_proposal: handoff_bytes.as_deref().map(Object::from_bytes),
    };
    let index = identity::canonical_json(&serde_json::to_value(set)?)?;
    if index.len() > BYTES as usize {
        bail!("native candidate index exceeds budget");
    }
    fs::create_dir("records")?;
    if let Some(bytes) = &handoff_bytes {
        write(handoff::FILE, bytes)?;
    }
    for (name, bytes) in bytes_by_path {
        write(&name, &bytes)?;
    }
    write(CLAIM, claim.bytes())?;
    write(INDEX, &index)?;
    Ok(Some(
        json!({"claim":claim.object(),"index":Object::from_bytes(&index),"record_count":prepared.templates.len(),"current_selection":false,"complete_semantic_coverage":false}),
    ))
}

pub(super) fn readback(
    prepared: &Prepared,
    profile: &Profile,
    plan: &JobPlan<'_>,
    phase: &PhaseResult,
    journal: &mut SourceJournal,
) -> Result<Option<Value>> {
    if prepared.specs.is_empty() {
        return Ok(None);
    }
    let verified = retained::verify(profile, phase, journal)?;
    let claim = tig_assets::graph::recipe::produced::Claim::parse(
        verified.object(CLAIM)?,
        &verified.bytes(phase, CLAIM, BYTES as usize)?,
    )?;
    let checked = claim.check_plan(plan.recipe())?;
    if claim.context().root != profile.worker.context.root
        || claim.context().identity != profile.worker.context.context
        || claim.outputs().len() != prepared.specs.len()
    {
        bail!("candidate claim context or subjects differ");
    }
    let set: CandidateSet = serde_json::from_value(identity::parse_json(&verified.bytes(
        phase,
        INDEX,
        BYTES as usize,
    )?)?)?;
    if set.schema != "reel.native-record-candidate-set.experimental-1"
        || &set.claim != claim.object()
        || &set.receipt != verified.object("native/receipt.json")?
        || set.records.len() != prepared.templates.len()
    {
        bail!("candidate index differs from retained artifacts");
    }
    let mut expected: BTreeMap<_, _> = prepared
        .templates
        .iter()
        .map(|(id, t)| (id.clone(), t.instance.clone()))
        .collect();
    let git_profile = if let Some(h) = &prepared.handoff {
        let bytes = h.profile(
            set.records
                .iter()
                .map(|r| (r.asset_id.as_str(), &r.template.object, &r.object)),
            &prepared.roots,
        )?;
        let object = Object::from_bytes(&bytes);
        if set.git_proposal.as_ref() != Some(&object)
            || verified.object(handoff::FILE)? != &object
            || verified.bytes(phase, handoff::FILE, BYTES as usize)? != bytes
        {
            bail!("retained native Git handoff differs");
        }
        Some(json!({"object":object,"retained_path":phase.value["outputs"][handoff::FILE]["path"]}))
    } else {
        if set.git_proposal.is_some() {
            bail!("unrequested native Git handoff");
        }
        None
    };
    for spec in &prepared.specs {
        assign(
            expected.get_mut(&spec.template.id).unwrap(),
            spec,
            verified.object(&format!("native/{}", spec.output))?,
        )?;
    }
    let mut seen = BTreeSet::new();
    let mut exported = vec![];
    for record in &set.records {
        if !seen.insert(&record.asset_id) {
            bail!("duplicate candidate record ID");
        }
        let template = prepared
            .templates
            .get(&record.asset_id)
            .ok_or_else(|| anyhow::anyhow!("foreign candidate record"))?;
        if record.template != template.reference
            || record.path != path(&record.asset_id)
            || &record.object != verified.object(&record.path)?
        {
            bail!("candidate template or retained record pin differs");
        }
        let bytes = verified.bytes(phase, &record.path, BYTES as usize)?;
        let bound = Instance::parse(&bytes)?.bind(&template.definition, &template.schema)?;
        let expected = expected.get_mut(&record.asset_id).unwrap();
        annotate(
            expected,
            template,
            &prepared.specs,
            claim.object(),
            &set.receipt,
        )?;
        if bound.instance().value() != expected {
            bail!("candidate changed fields outside admitted output updates");
        }
        let definition = Reference {
            id: bound.definition().value().id.clone(),
            object: bound.definition().object().clone(),
        };
        for spec in prepared
            .specs
            .iter()
            .filter(|s| s.template.id == record.asset_id)
        {
            let subject = BoundSubject::bind(&bound, &definition, spec.selector.clone())?
                .ok_or_else(|| anyhow::anyhow!("retained candidate subject missing"))?;
            if tig_assets::graph::recipe::produced::compare(
                plan.recipe(),
                Some(&subject),
                Some(&checked),
            )? != tig_assets::graph::recipe::produced::Correspondence::ClaimedRecipeMatch
            {
                bail!("retained candidate correspondence differs");
            }
        }
        exported.push(json!({"asset_id":record.asset_id,"template":record.template,"record":record.object,"retained_path":phase.value["outputs"][&record.path]["path"]}));
    }
    retained::owned_selection(
        journal,
        &Key::parse(&profile.source_id)?,
        &Key::parse(&profile.owner)?,
        phase.revision,
    )?;
    Ok(Some(
        json!({"schema":"reel.native-record-candidates-readback.experimental-1","claim":claim.object(),"receipt":set.receipt,"index":verified.object(INDEX)?,"records":exported,"git_proposal":git_profile,"current_selection":false,"complete_semantic_coverage":false}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> Spec {
        Spec {
            output: "master.mkv".into(),
            template: Reference {
                id: "delivery/output".into(),
                object: Object::from_bytes(b"template"),
            },
            selector: Selector::Selected {},
            stream_class: None,
        }
    }
    #[test]
    fn selector_aliases_conflicting_templates_and_stream_classes_refuse() {
        let one = spec();
        assert!(validate_specs(std::slice::from_ref(&one)).is_ok());
        assert!(validate_specs(&[one.clone(), one.clone()]).is_err());
        let mut stream = one.clone();
        stream.selector = Selector::Stream { slot: "es".into() };
        assert!(validate_specs(std::slice::from_ref(&stream)).is_err());
        stream.stream_class = Some(StreamClass {
            role: "delivery".into(),
            group: "language".into(),
        });
        assert!(validate_specs(&[one.clone(), stream.clone()]).is_ok());
        stream.template.object = Object::from_bytes(b"foreign template");
        assert!(validate_specs(&[one.clone(), stream]).is_err());
        let mut selected = one.clone();
        selected.stream_class = Some(StreamClass {
            role: "delivery".into(),
            group: "language".into(),
        });
        assert!(validate_specs(&[selected]).is_err());
        assert!(validate_specs(&vec![one; MAX_RECORDS + 1]).is_err());
    }
    #[test]
    fn inherited_stream_class_cannot_change_and_unmapped_history_is_preserved() {
        let schema = br#"{"type":"object"}"#.to_vec();
        let definition=serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/output","authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
        let raw=serde_json::to_vec(&json!({"schema":"tig.instance.design-1","id":"delivery/output","definition":{"id":"types/output","sha256":Object::from_bytes(&definition).sha256,"bytes":definition.len()},"authored":{},"selected":null,"streams":[{"slot":"es","role":"delivery","group":"language","object":Object::from_bytes(b"old")}],"status":{"approved":true},"metadata":{"old_review":true}})).unwrap();
        let bound = Instance::parse(&raw)
            .unwrap()
            .bind(&definition, &schema)
            .unwrap();
        let mut value = bound.instance().value().clone();
        let old = value.clone();
        let template = Template {
            reference: Reference {
                id: value.id.clone(),
                object: Object::from_bytes(&raw),
            },
            instance: old.clone(),
            definition,
            schema,
        };
        let mut target = spec();
        target.selector = Selector::Stream { slot: "es".into() };
        target.stream_class = Some(StreamClass {
            role: "other".into(),
            group: "language".into(),
        });
        assert!(assign(&mut value, &target, &Object::from_bytes(b"new")).is_err());
        target = spec();
        assign(&mut value, &target, &Object::from_bytes(b"new")).unwrap();
        annotate(
            &mut value,
            &template,
            std::slice::from_ref(&target),
            &Object::from_bytes(b"claim"),
            &Object::from_bytes(b"receipt"),
        )
        .unwrap();
        assert!(value.status.is_empty());
        assert!(!value.metadata.contains_key("old_review"));
        assert_eq!(value.streams, old.streams);
        bind(&value, &template).unwrap();
    }
}
