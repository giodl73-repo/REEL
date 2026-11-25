//! Native candidate ingress and retained reopen; not full task closure.
use super::common::{NOTE, returned};
use super::*;
use git_asset::annex::publication::{self, Publisher, Resources};
use git_asset_core::tree::NodeStore;
use git_asset_store::{
    existing::{Layout, Place, ReadCache, RequiredProtection},
    ledger::CapacityLedger,
    local::LocalStore,
};
use reel_workflow_git_asset::candidates::{Candidates, RETURN_PACKET};
fn exercise_return(bare: bool) {
    let mut f = Fixture::new(bare);
    let mut input_guard =
        GuardedInputs::new(f.profile.clone(), &f.source, &f.preview, 1024 * 1024).unwrap();
    input_guard.read(&f.artifact, f.artifact.bytes).unwrap();
    input_guard
        .admit_claim(&f.handle, &mut f.nodes, &limits())
        .unwrap();
    let root = f.temp.path();
    let ns_path = f.profile.members[0].root.clone();
    let mut ns = Namespace::open(&ns_path, f.handle.minimum()).unwrap();
    for (op, source, class) in [
        ("enroll-note", "return-note", Kind::Retention),
        ("enroll-packet", "return-packet", Kind::Retention),
        ("enroll-creative", "creative", Kind::Branch),
    ] {
        ns.apply(
            key(op),
            Command::Enroll {
                source: key(source),
                owner: key("owner"),
                class,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    }
    let (mut ticket, raw) = returned(&f.preview.commit, vec![f.artifact.clone()]);
    ticket.native_binding = common::artifact(
        "native/original",
        &f.handle.encode().unwrap(),
        "application/json",
    );
    let set = Candidates::bind(&ticket, &raw, "returns/attempt-1").unwrap();
    let staging = root.join("return-staging");
    fs::create_dir(&staging).unwrap();
    fs::write(
        staging.join(".git-asset-staging"),
        b"synthetic return stage",
    )
    .unwrap();
    let profile = publication::Profile {
        pool: f.profile.clone(),
        retained_place: key("pool"),
        staging,
        staging_marker: Object::from_bytes(b"synthetic return stage"),
        budget: publication::Budget {
            object_bytes: 1024 * 1024,
            transfer_bytes: 16 * 1024 * 1024,
            millis: 60000,
        },
        allow_publish: true,
    };
    let witness = LocalStore::create(&root.join("return-witness"), key("return-witness")).unwrap();
    let mut ledger = CapacityLedger::create(
        &root.join("return-ledger"),
        key("return-capacity"),
        key("epoch"),
        65536,
    )
    .unwrap();
    let probe = ledger
        .begin_probe(key("probe"))
        .unwrap()
        .reference()
        .clone();
    ledger
        .observe(key("observe"), &probe, fs2::available_space(root).unwrap())
        .unwrap();
    let job = ledger
        .reserve(key("return-job"), key("owner"), 32 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let metadata_root = root.join("return-metadata");
    let mut writer = MetadataStore::create(
        &metadata_root,
        metadata::Limits {
            nodes: 10000,
            bytes: 4 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let provenance = writer.put(set.provenance()).unwrap();
    let mut plans = BTreeMap::new();
    for (role, artifact) in set.expected() {
        let source = if role == RETURN_PACKET {
            "return-packet"
        } else {
            "return-note"
        };
        let request = publication::Request {
            operation: key(source),
            owner: key("owner"),
            source: key(source),
            expected_revision: 1,
            candidate: publication::Candidate {
                logical_id: artifact.logical_id.clone(),
                object: Object {
                    sha256: Digest::parse(&artifact.sha256).unwrap(),
                    bytes: artifact.bytes,
                },
                provenance: provenance.clone(),
            },
        };
        let frozen = publication::freeze(&request, &mut writer, TreeLimits::default()).unwrap();
        plans.insert(
            role.clone(),
            publication::Plan::new(
                &profile,
                request,
                frozen,
                publication::Admission {
                    namespace: ns.identity().clone(),
                    witness: witness.identity().clone(),
                    capacity: ledger.identity().clone(),
                    job: job.clone(),
                },
            )
            .unwrap(),
        );
    }
    let cp = writer.finish().unwrap();
    drop(writer);
    let mut metadata = MetadataReader::open(&metadata_root, &cp).unwrap();
    let prepared = set.prepare(plans).unwrap();
    let control = prepared.control().to_vec();
    let pin = Object::from_bytes(&control);
    let produced = root.join("returned");
    fs::create_dir(&produced).unwrap();
    fs::create_dir(produced.join("objects")).unwrap();
    fs::write(
        produced
            .join("objects")
            .join(Object::from_bytes(NOTE).sha256.as_str()),
        NOTE,
    )
    .unwrap();
    let mut source = ReadCache::new(existing::Profile {
        places: vec![Place {
            root: produced.clone(),
            layout: Layout::Flat,
        }],
        required: RequiredProtection::PreventMutation,
        limits: existing::Limits {
            object_bytes: 1024 * 1024,
            total_bytes: 8 * 1024 * 1024,
            requests: 16,
            live_readers: 8,
            verify_millis: 30000,
        },
    })
    .unwrap();
    let mut publisher = Publisher::new(profile.clone()).unwrap();
    let mut held = Vec::new();
    for name in prepared.plans().keys() {
        let mut resources = Resources {
            namespace: &mut ns,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        };
        let candidate = if name == RETURN_PACKET {
            prepared
                .apply(
                    name,
                    &mut publisher,
                    &mut resources,
                    &mut prepared.candidates().packet_source(),
                )
                .unwrap()
        } else {
            prepared
                .apply(name, &mut publisher, &mut resources, &mut source)
                .unwrap()
        };
        held.push(candidate);
    }
    let mut batch = prepared.gather(held).unwrap();
    assert_eq!(
        batch.acknowledgement().sha256,
        Object::from_bytes(&raw).sha256.as_str()
    );
    let recovery = batch.candidates().last().unwrap().1.recovery().clone();
    let mut bytes = vec![];
    batch
        .candidate_mut(RETURN_PACKET)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, raw);
    let mut bytes = vec![];
    batch
        .candidate_mut("output/note")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, NOTE);
    assert!(
        ns.source(&key("creative"))
            .unwrap()
            .unwrap()
            .current
            .is_none()
    );
    drop(batch);
    drop(source);
    drop(publisher);
    drop(input_guard);
    drop(ns);
    drop(metadata);
    drop(witness);
    drop(ledger);
    // Preserve original source bytes while making its old locator unavailable.
    fs::rename(&produced, root.join("returned-offline")).unwrap();
    fs::rename(
        root.join(if bare { "project.git" } else { "project" }),
        root.join("project-offline"),
    )
    .unwrap();
    let prepared = Candidates::bind(&ticket, &raw, "returns/attempt-1")
        .unwrap()
        .restore(&control, &pin)
        .unwrap();
    let witness = LocalStore::open(&root.join("return-witness")).unwrap();
    let mut ledger =
        CapacityLedger::open(&root.join("return-ledger"), &recovery.capacity_minimum).unwrap();
    let mut ns = Namespace::open(&ns_path, &recovery.namespace_minimum).unwrap();
    let mut metadata = MetadataReader::open(&metadata_root, &recovery.metadata_minimum).unwrap();
    let mut publisher = Publisher::new(profile).unwrap();
    let ns_before = ns.checkpoint();
    let ledger_before = ledger.checkpoint();
    let mut held = Vec::new();
    for name in prepared.plans().keys() {
        let mut resources = Resources {
            namespace: &mut ns,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        };
        held.push(
            prepared
                .reopen(name, &mut publisher, &mut resources)
                .unwrap(),
        );
    }
    let mut batch = prepared.gather(held).unwrap();
    let mut bytes = vec![];
    batch
        .candidate_mut(RETURN_PACKET)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, raw);
    let mut bytes = vec![];
    batch
        .candidate_mut("output/note")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, NOTE);
    assert!(
        ns.source(&key("creative"))
            .unwrap()
            .unwrap()
            .current
            .is_none()
    );
    assert_eq!(ns.checkpoint(), ns_before);
    assert_eq!(ledger.checkpoint(), ledger_before);
    // Completed retry must neither reopen the offline producer nor register twice.
    struct Offline {
        opens: usize,
    }
    impl existing::ByteSource for Offline {
        type Reader = std::io::Cursor<Vec<u8>>;
        fn open_verified(&mut self, _: &Object) -> existing::Result<Self::Reader> {
            self.opens += 1;
            Err(existing::Error::Missing)
        }
    }
    let mut offline = Offline { opens: 0 };
    let mut resources = Resources {
        namespace: &mut ns,
        metadata: &mut metadata,
        limits: limits(),
        witness: &witness,
        ledger: &mut ledger,
    };
    let retry = prepared
        .apply("output/note", &mut publisher, &mut resources, &mut offline)
        .unwrap();
    drop(retry);
    let recovered = prepared
        .resume("output/note", &mut publisher, &mut resources)
        .unwrap();
    drop(recovered);
    assert_eq!(offline.opens, 0);
    assert_eq!(ns.checkpoint(), ns_before);
    assert_eq!(ledger.checkpoint(), ledger_before);
    // Duplicate roles cannot satisfy complete retained output coverage.
    drop(batch);
    let mut duplicates = Vec::new();
    for _ in 0..2 {
        let mut resources = Resources {
            namespace: &mut ns,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        };
        duplicates.push(
            prepared
                .reopen("output/note", &mut publisher, &mut resources)
                .unwrap(),
        );
    }
    assert_eq!(
        prepared.gather(duplicates).err().unwrap().kind,
        ErrorKind::Conflict
    );
}
#[test]
#[ignore = "requires explicitly admitted native git and git-annex runtime"]
fn ordinary_return_candidates_reopen_retained_with_sources_offline() {
    exercise_return(false)
}
#[test]
#[ignore = "requires explicitly admitted native git and git-annex runtime"]
fn bare_return_candidates_reopen_retained_with_sources_offline() {
    exercise_return(true)
}
