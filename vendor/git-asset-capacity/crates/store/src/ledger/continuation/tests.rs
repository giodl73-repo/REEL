use super::*;
use crate::metadata::{Limits as MetadataLimits, MetadataStore};
use git_asset_core::capacity::CapacityError;
use std::collections::BTreeMap;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}

pub(super) struct Fixture {
    pub(super) _temp: tempfile::TempDir,
    pub(super) ledger: CapacityLedger,
    pub(super) nodes: MetadataStore,
    pub(super) data: PreparedData,
}
pub(super) fn fixture() -> Fixture {
    fixture_with_limits(MetadataLimits {
        nodes: 262144,
        bytes: 64 * 1024 * 1024,
        database_bytes: 96 * 1024 * 1024,
        min_free_bytes: 65536,
    })
}
pub(super) fn fixture_with_limits(limits: MetadataLimits) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let mut ledger =
        CapacityLedger::create(&temp.path().join("ledger"), key("volume"), key("epoch"), 10)
            .unwrap();
    let probe = ledger.begin_probe(key("probe")).unwrap();
    ledger
        .observe(key("sample"), probe.reference(), 10000)
        .unwrap();
    let job = ledger.reserve(key("job"), key("owner"), 1000).unwrap();
    ledger
        .begin_allocation(key("allocation"), job.reference(), key("pending"), 200)
        .unwrap();
    let mut nodes = MetadataStore::create(&temp.path().join("metadata"), limits).unwrap();
    let export = ledger
        .export_history(&mut nodes, ReadLimits::default())
        .unwrap();
    let (index, _) =
        Index::from_legacy(export.snapshot(), &mut nodes, TreeLimits::default()).unwrap();
    let (operations, sequences) = index.roots();
    let data = PreparedData {
        schema: "tig.capacity-successor-data.design-1".into(),
        config: ledger.config.clone(),
        generation: 1,
        source: ledger.checkpoint(),
        working_state: export.snapshot().working_state().clone(),
        operations: operations.clone(),
        sequences: sequences.clone(),
        activations: Tree::empty(&mut nodes).unwrap().root().clone(),
        operation_count: ledger.checkpoint().sequence,
        clock_high: None,
        predecessor: None,
    };
    Fixture {
        _temp: temp,
        ledger,
        nodes,
        data,
    }
}

// Wire-only construction: deliberately does not manufacture an observed SQL seal.
pub(super) fn control(f: &mut Fixture) -> ActiveControl {
    let object = f.nodes.put(&f.data.to_json().unwrap()).unwrap();
    let metadata = f.nodes.finish().unwrap();
    let preparation = Preparation {
        schema: "tig.capacity-successor-preparation.design-1".into(),
        operation: key("rotation"),
        data: object,
        metadata,
        tail: TailIdentity {
            authority: f.data.source.genesis.clone(),
            generation: f.data.generation,
            operation: key("rotation"),
            base: f.data.source.clone(),
        },
    };
    let intent = SealIntent {
        schema: format!(
            "tig.capacity-activation-intent.design-{}",
            if f.data.predecessor.is_none() { 1 } else { 2 }
        ),
        operation: preparation.operation.clone(),
        authority: preparation.tail.authority.clone(),
        generation: preparation.tail.generation,
        source: preparation.tail.base.clone(),
        prepared: Object::from_bytes(&preparation.to_json().unwrap()),
        predecessor: f.data.predecessor.clone(),
    };
    ActiveControl {
        schema: "tig.capacity-ledger.design-3".into(),
        preparation,
        seal: SealReceipt {
            hash: Digest::blob(&intent.to_json().unwrap()),
            intent,
        },
    }
}
pub(super) fn minimum(c: &ActiveControl) -> Minimum {
    Minimum {
        schema: "tig.capacity-successor-minimum.design-1".into(),
        authority: c.seal.intent.authority.clone(),
        activation: c.identity().unwrap(),
        checkpoint: c.seal.intent.source.clone(),
    }
}
pub(super) fn read(
    f: &Fixture,
    c: &ActiveControl,
    m: &Minimum,
    limits: ReadLimits,
) -> Result<Base> {
    let mut reader = f.nodes.finished_reader().unwrap();
    read_base(c, &f.data.source.genesis, m, &mut reader, limits)
}
fn ancestry(f: &mut Fixture, identities: &[ActivationIdentity]) {
    let entries: BTreeMap<_, _> = identities
        .iter()
        .map(|id| {
            let a = Ancestor {
                schema: "tig.capacity-successor-ancestor.design-1".into(),
                authority: f.data.source.genesis.clone(),
                generation: id.generation,
                digest: id.digest.clone(),
            };
            (
                ancestor_path(id.generation).unwrap(),
                f.nodes.put(&bytes(&a).unwrap()).unwrap(),
            )
        })
        .collect();
    f.data.activations = Tree::build(&entries, &mut f.nodes, TreeLimits::default())
        .unwrap()
        .0
        .root()
        .clone();
}

#[test]
fn finished_metadata_base_preserves_inflight_and_invalidates_probe_without_sql_commands() {
    let mut f = fixture();
    let head = f.ledger.checkpoint();
    let before = f.ledger.accounting().unwrap();
    let c = control(&mut f);
    let raw = c.to_json().unwrap();
    assert_eq!(ActiveControl::from_json(&raw).unwrap(), c);
    // Reopen a real finished provider after dropping the writer.
    let root = f._temp.path().join("metadata");
    let admitted = c.preparation.metadata.clone();
    drop(f.nodes);
    let mut reader = MetadataReader::open(&root, &admitted).unwrap();
    let m = minimum(&c);
    let mut base = read_base(&c, &m.authority, &m, &mut reader, ReadLimits::default()).unwrap();
    assert_eq!(base.data, f.data);
    assert_eq!(base.tail, TailRequirement::BaseVerified);
    assert!(base.metrics.calls > 0 && base.metrics.calls < 1024);
    assert_eq!(base.model.summary(), Err(CapacityError::UnknownObservation));
    let job = base.model.reserve("job", "owner", 1000).unwrap();
    assert_eq!(base.model.finish(&job), Err(CapacityError::InFlight));
    let pending = base.model.begin_allocation(&job, "pending", 200).unwrap();
    assert_eq!(base.model.allocation_status(&pending).unwrap(), None);
    assert_eq!(f.ledger.checkpoint(), head);
    assert_eq!(f.ledger.accounting().unwrap(), before);
    assert_eq!(reader.checkpoint(), &admitted);
}

#[test]
fn ancestor_minimum_and_predecessor_are_point_proven_even_with_current_minimum() {
    let mut f = fixture();
    let first = control(&mut f);
    let old = minimum(&first);
    f.data.generation = 2;
    f.data.predecessor = Some(old.activation.clone());
    ancestry(&mut f, std::slice::from_ref(&old.activation));
    let second = control(&mut f);
    let now = minimum(&second);
    assert_eq!(
        read(&f, &second, &old, ReadLimits::default()).unwrap().tail,
        TailRequirement::BaseVerified
    );
    assert!(read(&f, &second, &now, ReadLimits::default()).is_ok());
    let mut wrong = old.clone();
    wrong.activation.digest = Digest::blob(b"wrong");
    assert!(read(&f, &second, &wrong, ReadLimits::default()).is_err());
    let mut future = now.clone();
    future.activation.generation = 3;
    assert!(matches!(
        read(&f, &second, &future, ReadLimits::default()),
        Err(Error::Journal(LedgerError::Rollback))
    ));
    // Re-pinned but absent predecessor must fail even when minimum is current.
    ancestry(&mut f, &[]);
    let missing = control(&mut f);
    assert!(read(&f, &missing, &minimum(&missing), ReadLimits::default()).is_err());
    // The current generation may not be declared its own ancestor.
    ancestry(&mut f, &[old.activation, now.activation]);
    let self_entry = control(&mut f);
    assert!(
        read(
            &f,
            &self_entry,
            &minimum(&self_entry),
            ReadLimits::default()
        )
        .is_err()
    );
}

#[test]
fn above_base_checkpoint_requires_actual_tail_proof_and_cannot_be_admitted_here() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut m = minimum(&c);
    m.checkpoint.sequence += 1;
    m.checkpoint.hash = Digest::blob(b"unverified tail hash");
    assert_eq!(
        read(&f, &c, &m, ReadLimits::default()).unwrap().tail,
        TailRequirement::AtLeast(m.checkpoint.clone())
    );
    m.checkpoint.sequence = f.data.source.sequence + MAX_COMMANDS + 1;
    assert!(matches!(
        read(&f, &c, &m, ReadLimits::default()),
        Err(Error::Journal(LedgerError::Rollback))
    ));
    m.checkpoint = f.data.source.clone();
    m.checkpoint.hash = Digest::blob(b"incorrect prefix");
    assert!(matches!(
        read(&f, &c, &m, ReadLimits::default()),
        Err(Error::Journal(LedgerError::Rollback))
    ));
    m.checkpoint.sequence = 0;
    m.checkpoint.hash = m.checkpoint.genesis.clone();
    assert_eq!(
        read(&f, &c, &m, ReadLimits::default()).unwrap().tail,
        TailRequirement::BaseVerified
    );
}

#[test]
fn foreign_empty_working_state_is_rejected_after_coherent_metadata_repin() {
    let mut f = fixture();
    let original = f.data.working_state.clone();
    for (epoch, floor) in [
        ("foreign".to_owned(), f.data.config.floor),
        (
            f.data.config.instance.as_str().to_owned(),
            f.data.config.floor + 1,
        ),
    ] {
        let foreign = CapacityDomain::new(f.data.config.domain.as_str(), &epoch, floor).unwrap();
        f.data.working_state = foreign
            .export_checkpoint(&mut f.nodes, state::Limits::default())
            .unwrap()
            .0;
        let c = control(&mut f);
        assert!(matches!(
            read(&f, &c, &minimum(&c), ReadLimits::default()),
            Err(Error::State(state::Error::Invalid))
        ));
    }
    f.data.working_state = original;
    let c = control(&mut f);
    assert!(read(&f, &c, &minimum(&c), ReadLimits::default()).is_ok());
}

#[test]
fn missing_nodes_and_foreign_authority_hold_even_after_valid_control_repin() {
    let mut f = fixture();
    let c = control(&mut f);
    let mut m = minimum(&c);
    m.authority = Digest::blob(b"foreign authority");
    m.checkpoint.genesis = m.authority.clone();
    assert!(read(&f, &c, &m, ReadLimits::default()).is_err());
    // A valid bounded root descriptor is not evidence the referenced node exists.
    f.data.activations = Object::from_bytes(b"absent ancestry node");
    let missing = control(&mut f);
    assert!(matches!(
        read(&f, &missing, &minimum(&missing), ReadLimits::default()),
        Err(Error::Missing(_))
    ));
    // The prepared minimum cannot name a later commit than the opened reader.
    let old_reader = f.nodes.finished_reader().unwrap();
    f.nodes.put(b"later node").unwrap();
    f.nodes.finish().unwrap();
    let newer = control(&mut f);
    let m = minimum(&newer);
    let mut reader = old_reader;
    let before = reader.metrics();
    assert!(read_base(&newer, &m.authority, &m, &mut reader, ReadLimits::default()).is_err());
    assert_eq!(reader.metrics(), before);
}

#[test]
fn legacy_intent_keeps_original_fields_and_sequence_cap() {
    let mut f = fixture();
    let c = control(&mut f);
    let raw = c.seal.intent.to_json().unwrap();
    let legacy: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(legacy.as_object().unwrap().len(), 6);
    assert!(legacy.get("predecessor").is_none());
    assert_eq!(record::<SealIntent>(&raw).unwrap(), c.seal.intent);
    let mut intent = c.seal.intent.clone();
    intent.source.sequence = MAX_COMMANDS + 1;
    assert!(intent.to_json().is_err());
    intent.schema = "tig.capacity-activation-intent.design-2".into();
    intent.generation = 2;
    intent.predecessor = Some(c.identity().unwrap());
    assert!(intent.to_json().is_ok());
    intent.generation += 1;
    assert!(intent.to_json().is_err());
}

#[test]
fn read_budget_and_metadata_identity_fail_before_unadmitted_provider_reads() {
    let mut f = fixture();
    let c = control(&mut f);
    let m = minimum(&c);
    for limits in [
        ReadLimits {
            max_calls: 0,
            ..ReadLimits::default()
        },
        ReadLimits {
            max_bytes: c.preparation.data.bytes as usize - 1,
            ..ReadLimits::default()
        },
    ] {
        let mut reader = f.nodes.finished_reader().unwrap();
        let before = reader.metrics();
        assert!(matches!(
            read_base(&c, &m.authority, &m, &mut reader, limits),
            Err(Error::Budget)
        ));
        assert_eq!(reader.metrics(), before);
    }
    assert!(matches!(
        read(
            &f,
            &c,
            &m,
            ReadLimits {
                max_calls: 1,
                ..ReadLimits::default()
            }
        ),
        Err(Error::Budget)
    ));
    let other = fixture();
    let mut reader = other.nodes.finished_reader();
    // The unrelated provider must be finished before it can be admitted.
    assert!(reader.is_err());
    let mut other = other;
    other.nodes.finish().unwrap();
    reader = other.nodes.finished_reader();
    let mut reader = reader.unwrap();
    let before = reader.metrics();
    assert!(read_base(&c, &m.authority, &m, &mut reader, ReadLimits::default()).is_err());
    assert_eq!(reader.metrics(), before);
}

#[test]
fn codecs_reject_extra_noncanonical_rebound_and_legacy_null_predecessor() {
    let mut f = fixture();
    let c = control(&mut f);
    let raw = c.to_json().unwrap();
    assert!(ActiveControl::from_json(&[b" ".as_slice(), raw.as_slice()].concat()).is_err());
    assert!(ActiveControl::from_json(&vec![b' '; 65537]).is_err());
    let mut json: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    json["extra"] = true.into();
    assert!(ActiveControl::from_json(&encode(&json).unwrap()).is_err());
    let mut json = serde_json::to_value(&c.seal.intent).unwrap();
    json["predecessor"] = serde_json::Value::Null;
    assert!(record::<SealIntent>(&encode(&json).unwrap()).is_err());
    for n in 0..5 {
        let mut bad = c.clone();
        match n {
            0 => bad.seal.hash = Digest::blob(b"wrong"),
            1 => bad.preparation.operation = key("other"),
            2 => bad.preparation.tail.generation += 1,
            3 => bad.seal.intent.prepared = Object::from_bytes(b"wrong"),
            _ => bad.preparation.tail.base.hash = Digest::blob(b"wrong"),
        }
        assert!(bad.to_json().is_err());
    }
    let mut data = f.data.clone();
    data.config.instance = Digest::blob(b"wrong-instance");
    assert!(data.to_json().is_err());
    data = f.data.clone();
    data.operation_count += 1;
    assert!(data.to_json().is_err());
    data = f.data.clone();
    data.clock_high = Some(1);
    assert!(data.to_json().is_err());
}

#[test]
fn provider_relative_names_and_ancestor_literal_have_fixed_independent_hashes() {
    // SHA256 vectors calculated independently from literal UTF8 using Python hashlib.
    assert_eq!(
        Digest::blob(b"rotation").as_str(),
        "224610f102890bc0e40c49ffb456bb93d45a6dee88dc9a7bef351fa10d3f8582"
    );
    let mut f = fixture();
    let c = control(&mut f);
    assert_eq!(
        c.preparation.tail.relative_name().unwrap(),
        "segments/00000000000000000001-224610f102890bc0e40c49ffb456bb93d45a6dee88dc9a7bef351fa10d3f8582.sqlite"
    );
    assert_eq!(
        c.preparation.relative_name().unwrap(),
        "activations/224610f102890bc0e40c49ffb456bb93d45a6dee88dc9a7bef351fa10d3f8582.json"
    );
    assert_eq!(
        c.relative_name().unwrap(),
        format!(
            "controls/{}.json",
            Digest::blob(&c.to_json().unwrap()).as_str()
        )
    );
    let raw = br#"{"authority":"0000000000000000000000000000000000000000000000000000000000000000","digest":"1111111111111111111111111111111111111111111111111111111111111111","generation":1,"schema":"tig.capacity-successor-ancestor.design-1"}"#;
    assert_eq!(
        Digest::blob(raw).as_str(),
        "b76607374ecac7ff97fc897eca6f41b987e733b082fa10a39883479bb4825ed8"
    );
    let a: Ancestor = record(raw).unwrap();
    assert_eq!(bytes(&a).unwrap(), raw);
}
