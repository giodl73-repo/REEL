//! Pure original task-binding correspondence; no byte custody/admission claims.
#[path = "common/mod.rs"]
mod common;
use common::artifact;
use git_asset_core::{
    Digest, LogicalPath, Object,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use git_asset_store::{
    Key,
    protection::Limits,
    source_claims::{
        Command, Kind, Places, Selection,
        admission::{Namespace, Policy, Select},
    },
};
use reel_workflow::{Artifact, ErrorKind, Packet};
use reel_workflow_git_asset::{
    Input, Preview,
    binding::{self, Binding, Capture},
    claims::Handle,
    retention::Seed,
};
use std::collections::BTreeMap;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn limits() -> Limits {
    Limits {
        millis: 60000,
        roots: 128,
        bindings: 4096,
        associations: 10000,
        reads: 10000,
        read_bytes: 4 * 1024 * 1024,
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    packet: Packet,
    raw: Vec<u8>,
    preview: Preview,
    claim: Handle,
    controls: BTreeMap<String, Artifact>,
    control: Seed,
    result: Seed,
    retained: Seed,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let policy = Policy {
            prefer_active: true,
            classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .map(|k| {
                    (
                        k,
                        Places {
                            current: vec![key("pool")],
                            pending: vec![key("pool")],
                            predecessor: vec![key("pool")],
                        },
                    )
                })
                .collect(),
        };
        let mut ns =
            Namespace::create_fenced(&temp.path().join("ns"), key("p"), key("e"), policy).unwrap();
        let mut nodes = MemoryNodes::default();
        for (source, kind) in [
            ("job", Kind::Job),
            ("controls", Kind::Retention),
            ("results", Kind::Retention),
            ("retained", Kind::Retention),
        ] {
            ns.apply(
                key(&format!("enroll-{source}")),
                Command::Enroll {
                    source: key(source),
                    owner: key("owner"),
                    class: kind,
                },
                &nodes,
                &limits(),
            )
            .unwrap();
        }
        let input = artifact(
            "synthetic/input",
            b"input bytes",
            "application/octet-stream",
        );
        let (ticket, _) = common::returned(&"1".repeat(40), vec![input.clone()]);
        let packet = ticket.packet;
        let mut raw = serde_json::to_vec_pretty(&packet).unwrap();
        raw.push(b'\n');
        let preview = Preview {
            schema: "reel.workflow-public-git-asset-inputs.v1".into(),
            commit: packet.source_commit.clone(),
            checkpoint: Digest::blob(b"metadata checkpoint"),
            inputs: vec![Input {
                artifact: input.clone(),
                record: Object::from_bytes(b"synthetic record"),
                object: Object::from_bytes(b"input bytes"),
                stream: None,
            }],
            logical_bytes: input.bytes,
        };
        let controls = BTreeMap::from([
            (
                "packet".into(),
                artifact("control/packet", &raw, "application/json"),
            ),
            (
                "request".into(),
                artifact("control/request", b"original request bytes", "text/plain"),
            ),
            (
                "preview".into(),
                artifact(
                    "control/preview",
                    &preview.encode().unwrap(),
                    "application/json",
                ),
            ),
            ("tool".into(), ticket.lifecycle_tool),
        ]);
        let entries = binding::resources(&packet, &preview, &controls).unwrap();
        let (tree, _) = Tree::build(&entries, &mut nodes, TreeLimits::default()).unwrap();
        let claim = Handle::prepare(
            &mut ns,
            Select {
                prepare: key("prepare-job"),
                adopt: key("adopt-job"),
                source: key("job"),
                owner: key("owner"),
                expected_revision: 1,
                selection: Selection {
                    root: tree.root().clone(),
                    context: Object::from_bytes(&preview.encode().unwrap()).sha256,
                },
            },
        )
        .unwrap();
        let control = Seed::prepare(
            &mut ns,
            key("controls"),
            key("owner"),
            key("prepare-controls"),
            key("adopt-controls"),
        )
        .unwrap();
        let result = Seed::prepare(
            &mut ns,
            key("results"),
            key("owner"),
            key("prepare-results"),
            key("adopt-results"),
        )
        .unwrap();
        let retained = Seed::prepare(
            &mut ns,
            key("retained"),
            key("owner"),
            key("prepare-retained"),
            key("adopt-retained"),
        )
        .unwrap();
        Self {
            _temp: temp,
            packet,
            raw,
            preview,
            claim,
            controls,
            control,
            result,
            retained,
        }
    }
    fn capture(&self) -> reel_workflow::Result<Binding> {
        Binding::capture(Capture {
            binding_id: "bindings/attempt-1",
            packet: &self.packet,
            original_packet: &self.raw,
            attempt: "attempt-1",
            preview: &self.preview,
            claim: &self.claim,
            controls: self.controls.clone(),
            control_seed: &self.control,
            result_seed: &self.result,
            retained_seed: &self.retained,
        })
    }
}
#[test]
fn immutable_binding_has_no_self_hash_and_preserves_raw_original_packet() {
    let f = Fixture::new();
    let b = f.capture().unwrap();
    let pin = b.pin();
    let decoded = Binding::decode(b.control(), &pin).unwrap();
    let ticket = decoded.ticket(f.packet.clone()).unwrap();
    decoded.verify(&ticket, &f.preview, &f.raw).unwrap();
    assert_eq!(decoded.claim(), &f.claim);
    let mut nodes = MemoryNodes::default();
    let request = decoded.control_selection(&mut nodes).unwrap();
    let (entries, _) = Tree::from_root(request.selection.root)
        .entries(&mut nodes, TreeLimits::default())
        .unwrap();
    assert_eq!(
        entries.get(&LogicalPath::parse("binding").unwrap()),
        Some(&pin)
    );
    assert_eq!(decoded.pin(), pin);
    assert_eq!(request.selection.context, pin.sha256);
    let mut normalized = f.raw.clone();
    normalized.push(b' ');
    assert!(
        matches!(decoded.verify(&ticket,&f.preview,&normalized),Err(e) if e.kind==ErrorKind::Conflict)
    );
}
#[test]
fn binding_rejects_changed_identity_attempt_tool_and_missing_controls() {
    let mut f = Fixture::new();
    let b = f.capture().unwrap();
    let ticket = b.ticket(f.packet.clone()).unwrap();
    for field in 0..3 {
        let mut changed = ticket.clone();
        match field {
            0 => changed.native_binding.logical_id = "bindings/different".into(),
            1 => changed.attempt_id = "different".into(),
            _ => changed.lifecycle_tool.logical_id = "tools/different".into(),
        };
        assert!(
            matches!(b.verify(&changed,&f.preview,&f.raw),Err(e) if e.kind==ErrorKind::Conflict)
        );
    }
    f.controls.remove("request");
    assert!(matches!(f.capture(),Err(e) if e.kind==ErrorKind::Budget||e.kind==ErrorKind::Missing));
    let huge = vec![b' '; 128 * 1024 + 1];
    assert!(
        matches!(Binding::decode(&huge,&Object::from_bytes(&huge)),Err(e) if e.kind==ErrorKind::Budget)
    );
    assert!(
        matches!(Binding::decode(b.control(),&Object::from_bytes(b"wrong pin")),Err(e) if e.kind==ErrorKind::Integrity)
    );
}
#[test]
fn binding_requires_exact_job_resource_closure_and_distinct_retention_sources() {
    let mut f = Fixture::new();
    f.controls.insert(
        "extra".into(),
        artifact("control/extra", b"not admitted in job root", "text/plain"),
    );
    assert!(matches!(f.capture(),Err(e) if e.kind==ErrorKind::Conflict));
    let f = Fixture::new();
    assert!(
        matches!(Binding::capture(Capture{binding_id:"bindings/attempt-1",packet:&f.packet,original_packet:&f.raw,attempt:"attempt-1",preview:&f.preview,claim:&f.claim,controls:f.controls.clone(),control_seed:&f.control,result_seed:&f.control,retained_seed:&f.retained}),Err(e) if e.kind==ErrorKind::Conflict)
    );
    let mut changed = f.preview.clone();
    changed.inputs.clear();
    assert!(binding::resources(&f.packet, &changed, &f.controls).is_err());
}
