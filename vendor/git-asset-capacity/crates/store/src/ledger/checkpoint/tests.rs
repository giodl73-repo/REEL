use super::*;
use crate::metadata::{Limits as MetadataLimits, MetadataReader, MetadataStore};
use git_asset_core::{
    capacity::CapacityError,
    tree::{MemoryNodes, TreeError, TreeResult},
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn setup(root: &Path) -> (CapacityLedger, super::super::Receipt, super::super::Receipt) {
    let mut l = CapacityLedger::create(root, key("volume"), key("epoch"), 10).unwrap();
    let p = l.begin_probe(key("probe")).unwrap();
    l.observe(key("sample"), p.reference(), 10_000).unwrap();
    let job = l.reserve(key("job"), key("owner"), 1000).unwrap();
    let a = l
        .begin_allocation(key("allocation"), job.reference(), key("data"), 200)
        .unwrap();
    let done = l
        .complete_allocation(key("complete"), a.reference(), 100)
        .unwrap();
    l.begin_allocation(key("pending"), job.reference(), key("unfinished"), 400)
        .unwrap();
    (l, a, done)
}
#[test]
fn actual_ledger_export_roundtrips_through_reopened_metadata_without_commands_or_new_probe() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, allocation, done) = setup(&root);
    let before = l.checkpoint();
    let summary = l.accounting().unwrap();
    let config = l.config.clone();
    let meta = temp.path().join("metadata");
    let mut nodes = MetadataStore::create(
        &meta,
        MetadataLimits {
            nodes: 65_536,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65_536,
        },
    )
    .unwrap();
    let export = l
        .export_checkpoint(&mut nodes, state::Limits::default())
        .unwrap();
    assert_eq!(export.descriptor().journal(), &before);
    assert!(export.metrics().calls > 0);
    assert_eq!(l.checkpoint(), before);
    assert_eq!(l.accounting().unwrap(), summary);
    let encoded = export.descriptor().to_json().unwrap();
    assert!(encoded.len() < 1024);
    let descriptor = Descriptor::from_json(&encoded).unwrap();
    assert_eq!(&descriptor, export.descriptor());
    let foreign = git_asset_core::capacity::CapacityDomain::new(
        config.domain.as_str(),
        "foreign-instance",
        config.floor + 1,
    )
    .unwrap();
    let (foreign_state, _) = foreign
        .export_checkpoint(&mut nodes, state::Limits::default())
        .unwrap();
    let minimum = nodes.finish().unwrap();
    drop(nodes);
    drop(l);
    let mut reader = MetadataReader::open(&meta, &minimum).unwrap();
    let decoded = descriptor
        .state()
        .read(&mut reader, state::Limits::default())
        .unwrap();
    assert_eq!(decoded.recorded_accounting().unwrap(), summary);
    let io = reader.metrics();
    let metrics = decoded.metrics();
    assert!(decoded.matches_context(
        config.domain.as_str(),
        config.instance.as_str(),
        config.floor
    ));
    assert!(!decoded.matches_context(config.domain.as_str(), config.epoch.as_str(), config.floor));
    assert!(!decoded.matches_context(
        config.domain.as_str(),
        config.instance.as_str(),
        config.floor + 1
    ));
    assert_eq!(reader.metrics(), io);
    assert_eq!(decoded.metrics(), metrics);
    let mut model = decoded
        .into_domain_for_context(
            config.domain.as_str(),
            config.instance.as_str(),
            config.floor,
        )
        .unwrap();
    let foreign = foreign_state
        .read(&mut reader, state::Limits::default())
        .unwrap();
    assert!(matches!(
        foreign.into_domain_for_context(
            config.domain.as_str(),
            config.instance.as_str(),
            config.floor
        ),
        Err(state::Error::Invalid)
    ));
    assert_eq!(reader.checkpoint(), &minimum);
    assert_eq!(model.summary(), Err(CapacityError::UnknownObservation));
    let job = model.reserve("job", "owner", 1000).unwrap();
    let pending = model.begin_allocation(&job, "unfinished", 400).unwrap();
    assert_eq!(model.allocation_status(&pending).unwrap(), None);
    assert_eq!(model.finish(&job), Err(CapacityError::InFlight));
    let mut l = CapacityLedger::open(&root, &before).unwrap();
    let replay = l
        .complete_allocation(key("complete"), allocation.reference(), 100)
        .unwrap();
    assert!(replay.was_replayed());
    assert_eq!(replay.checkpoint(), done.checkpoint());
    assert_eq!(replay.reference(), done.reference());
    assert_eq!(l.checkpoint(), before);
    assert!(matches!(
        l.reserve(key("new-job"), key("owner"), 1),
        Err(LedgerError::NeedsProbe)
    ));
}
struct PeerNodes {
    inner: MemoryNodes,
    peer: CapacityLedger,
    advanced: bool,
}
impl NodeStore for PeerNodes {
    fn get(&self, o: &git_asset_core::Object) -> TreeResult<Option<Vec<u8>>> {
        self.inner.get(o)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<git_asset_core::Object> {
        if !self.advanced {
            self.peer
                .invalidate(key("peer-change"))
                .map_err(|e| TreeError::Store(e.to_string()))?;
            self.advanced = true;
        }
        self.inner.put(b)
    }
}
#[test]
fn peer_can_commit_inside_metadata_io_and_changed_head_refuses_current_descriptor() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, _, _) = setup(&root);
    let before = l.checkpoint();
    let peer = CapacityLedger::open(&root, &before).unwrap();
    let mut nodes = PeerNodes {
        inner: MemoryNodes::default(),
        peer,
        advanced: false,
    };
    let result = l.export_checkpoint(&mut nodes, state::Limits::default());
    assert!(matches!(result, Err(Error::Journal(LedgerError::Conflict))));
    assert!(nodes.advanced);
    assert_eq!(l.checkpoint().sequence(), before.sequence() + 1);
    assert!(nodes.inner.objects().next().is_some());
    let current = l.checkpoint();
    let mut stable = MemoryNodes::default();
    let exported = l
        .export_checkpoint(&mut stable, state::Limits::default())
        .unwrap();
    assert_eq!(exported.descriptor().journal(), &current);
    assert_eq!(l.checkpoint(), current);
}
#[test]
fn failed_export_and_descriptor_tampering_cannot_reset_or_activate_a_ledger() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, _, _) = setup(&root);
    let before = l.checkpoint();
    let summary = l.accounting().unwrap();
    let mut nodes = MemoryNodes::default();
    let result = l.export_checkpoint(
        &mut nodes,
        state::Limits {
            max_nodes: 1,
            ..state::Limits::default()
        },
    );
    assert!(matches!(result, Err(Error::State(state::Error::Budget))));
    assert_eq!(l.checkpoint(), before);
    assert_eq!(l.accounting().unwrap(), summary);
    let valid = l
        .export_checkpoint(&mut nodes, state::Limits::default())
        .unwrap();
    let bytes = valid.descriptor().to_json().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["schema"] = "future".into();
    assert!(Descriptor::from_json(&identity::canonical_json(&value).unwrap()).is_err());
    value["schema"] = "tig.capacity-working-checkpoint.design-1".into();
    value["unrecognized"] = true.into();
    assert!(Descriptor::from_json(&identity::canonical_json(&value).unwrap()).is_err());
    assert!(Descriptor::from_json(&[bytes, b" ".to_vec()].concat()).is_err());
    assert_eq!(l.checkpoint(), before);
}
