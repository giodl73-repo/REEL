use reel::{
    tig_inputs::Source,
    tig_publication::{ExistingPublisher, Profile},
    tig_scene_production::{
        OutputBinding, Publication,
        jobs::{Coordination, Identity, JobPlan},
    },
};
use reel_assets::{Config, PathPolicy};
use std::{fs, path::Path};
use tig_assets::graph::recipe::produced::BoundSubject;
use tig_core::{
    Digest, Object,
    tree::{NodeStore, TreeLimits, TreeResult},
};
use tig_git::source::native::job::Role;
use tig_store::{
    AckLevel, Key,
    existing::RequiredProtection,
    metadata::{Checkpoint, MetadataReader, MetadataStore},
    source_claims::{Command, SourceJournal},
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
struct Missing<'a> {
    inner: &'a MetadataReader,
    object: Object,
}
impl NodeStore for Missing<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if *object == self.object {
            Ok(None)
        } else {
            self.inner.get(object)
        }
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("job readback must not write")
    }
}

pub fn exercise(
    plan: &JobPlan<'_>,
    source: &mut Source,
    owners: &mut SourceJournal,
    root: &Path,
    metadata: (&Path, &Checkpoint),
    bind_as: impl Fn(&str, &Object) -> BoundSubject,
) {
    let coord_root = root.join("managed-coordination");
    fs::create_dir(&coord_root).unwrap();
    let coordination = Coordination {
        root: coord_root.clone(),
    };
    let identity = Identity {
        source: key("managed-job"),
        owner: key("managed-owner"),
    };
    let mut writer = MetadataStore::open(metadata.0, metadata.1).unwrap();
    let frozen = plan.freeze(&mut writer, TreeLimits::default()).unwrap();
    let witness = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(metadata.0, &witness).unwrap();
    assert!(
        plan.readback(
            frozen.selection(),
            &mut Missing {
                inner: &reader,
                object: plan.recipe().operation().producer.clone()
            },
            TreeLimits::default()
        )
        .is_err()
    );
    assert!(
        frozen
            .of(Role::Runtime)
            .contains(&plan.recipe().operation().producer)
    );
    assert!(plan.runtime().len() >= 3);
    for object in plan.runtime().values() {
        assert!(frozen.of(Role::Runtime).contains(object));
    }
    let ready = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap();
    assert!(!ready.resources().complete_claim_coverage());
    let job = ready
        .protect(
            owners,
            &coordination,
            identity.clone(),
            0,
            &key("managed-start"),
        )
        .unwrap();
    assert_eq!(job.revision(), 2);
    assert!(
        plan.readback(frozen.selection(), &mut reader, TreeLimits::default())
            .unwrap()
            .protect(
                owners,
                &coordination,
                identity.clone(),
                0,
                &key("managed-start")
            )
            .is_err()
    );
    let peer_identity = Identity {
        source: key("managed-peer"),
        owner: key("peer-owner"),
    };
    let mut peer = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap()
        .protect(
            owners,
            &coordination,
            peer_identity.clone(),
            0,
            &key("peer-start"),
        )
        .unwrap();
    assert_eq!(peer.revision(), 2);
    let before = owners.checkpoint();
    drop(job);
    let foreign = Identity {
        source: identity.source.clone(),
        owner: key("foreign-owner"),
    };
    assert!(
        plan.readback(frozen.selection(), &mut reader, TreeLimits::default())
            .unwrap()
            .resume(owners, &coordination, foreign, 2)
            .is_err()
    );
    let resumed = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap()
        .resume(owners, &coordination, identity.clone(), 2)
        .unwrap();
    assert_eq!(owners.checkpoint(), before);
    drop(resumed);
    let mut job = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap()
        .protect(
            owners,
            &coordination,
            identity.clone(),
            0,
            &key("managed-start"),
        )
        .unwrap();
    assert_eq!(owners.checkpoint(), before);
    let stale_identity = Identity {
        source: key("managed-stale"),
        owner: key("stale-owner"),
    };
    let mut stale = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap()
        .protect(
            owners,
            &coordination,
            stale_identity.clone(),
            0,
            &key("stale-start"),
        )
        .unwrap();
    owners
        .apply(
            key("external-withdraw"),
            Command::Withdraw {
                source: stale_identity.source,
                owner: stale_identity.owner,
                expected_revision: 2,
            },
        )
        .unwrap();
    assert!(
        stale
            .render(
                owners,
                source,
                &root.join("must-not-create-stale-job"),
                &root.join("must-not-create-stale-output")
            )
            .is_err()
    );
    assert!(!root.join("must-not-create-stale-job").exists());
    drop(stale);

    let candidate = job
        .render(
            owners,
            source,
            &root.join("managed-native-job"),
            &root.join("managed-native-output"),
        )
        .unwrap();
    let output = &candidate.candidate().receipt().outputs["master.mkv"];
    let pin = Object {
        sha256: Digest::parse(&output.sha256).unwrap(),
        bytes: output.bytes,
    };
    let subject = bind_as("delivery/managed-scene", &pin);
    let mapping = || OutputBinding {
        output_name: "master.mkv",
        subject: &subject,
    };
    let cache = root.join("managed-cache");
    fs::create_dir_all(cache.join("objects/sha256")).unwrap();
    let mut storage = Config {
        path_policy: PathPolicy::ConfiguredLocalRoots,
        cache_roots: vec![cache.clone()],
        coordination_root: Some(coord_root.clone()),
        ..Default::default()
    };
    storage.capacity.minimum_free_bytes = 0;
    storage.capacity.minimum_free_basis_points = 0;
    let mut backend = ExistingPublisher::open(Profile {
        artifact_root: root.to_owned(),
        namespace: key("managed-publication"),
        storage,
        destination: cache.clone(),
        reader_limits: Default::default(),
        protection: RequiredProtection::PreventMutation,
    })
    .unwrap();
    let publication = Publication {
        operation: key("managed-render-output"),
        owner: identity.owner.clone(),
        required: AckLevel::ProcessRestart,
    };
    assert!(
        peer.publish(owners, &candidate, &[mapping()], &publication, &mut backend)
            .is_err()
    );
    let mut other_owners = SourceJournal::create(
        &root.join("managed-other-journal"),
        key("fixture"),
        key("epoch"),
    )
    .unwrap();
    assert_ne!(owners.identity(), other_owners.identity());
    let mut other = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap()
        .protect(
            &mut other_owners,
            &coordination,
            identity.clone(),
            0,
            &key("managed-start"),
        )
        .unwrap();
    assert_eq!(other.revision(), job.revision());
    assert!(
        other
            .publish(
                &mut other_owners,
                &candidate,
                &[mapping()],
                &publication,
                &mut backend
            )
            .is_err()
    );
    assert!(
        job.render(
            &mut other_owners,
            source,
            &root.join("must-not-create-foreign-job"),
            &root.join("must-not-create-foreign-output")
        )
        .is_err()
    );
    assert!(!root.join("must-not-create-foreign-job").exists());
    drop(other);
    assert!(!coord_root.join("tig-publications").exists());
    let published = job
        .publish(owners, &candidate, &[mapping()], &publication, &mut backend)
        .unwrap();
    assert_eq!(published.published().acknowledgements().len(), 3);
    assert!(
        peer.freeze_outputs(
            owners,
            &published,
            &mut Missing {
                inner: &reader,
                object: pin.clone()
            },
            TreeLimits::default()
        )
        .is_err()
    );
    let mut writer = MetadataStore::open(metadata.0, &witness).unwrap();
    let output_candidate = job
        .freeze_outputs(owners, &published, &mut writer, TreeLimits::default())
        .unwrap();
    let output_witness = writer.finish().unwrap();
    drop(writer);
    let mut outputs_reader = MetadataReader::open(metadata.0, &output_witness).unwrap();
    let ready = job
        .readback_outputs(
            owners,
            &published,
            output_candidate.selection(),
            &mut outputs_reader,
            TreeLimits::default(),
        )
        .unwrap();
    assert!(ready.resources().of(Role::ClaimedOutput).contains(&pin));
    assert!(!ready.resources().of(Role::Primary).contains(&pin));
    let other_before = other_owners.checkpoint();
    assert!(
        job.adopt_outputs(&mut other_owners, &ready, &key("managed-output-handoff"))
            .is_err()
    );
    assert_eq!(other_owners.checkpoint(), other_before);
    job.adopt_outputs(owners, &ready, &key("managed-output-handoff"))
        .unwrap();
    assert_eq!(job.revision(), 4);
    assert_eq!(job.selection(), output_candidate.selection());
    let completed = owners.checkpoint();
    job.adopt_outputs(owners, &ready, &key("managed-output-handoff"))
        .unwrap();
    assert_eq!(owners.checkpoint(), completed);
    assert!(
        job.adopt_outputs(owners, &ready, &key("different-handoff"))
            .is_err()
    );
    assert!(
        job.render(
            owners,
            source,
            &root.join("must-not-render-after-handoff"),
            &root.join("must-not-output-after-handoff")
        )
        .is_err()
    );
    assert!(!root.join("must-not-render-after-handoff").exists());
    drop(job);
    drop(peer);
    drop(backend);
    let current = owners.source(&identity.source).unwrap().unwrap();
    assert_eq!(
        current.current.unwrap().selection,
        *output_candidate.selection()
    );
    assert_eq!(current.predecessors.len(), 1);
    assert_eq!(
        owners
            .source(&peer_identity.source)
            .unwrap()
            .unwrap()
            .revision,
        2
    );
    assert!(
        plan.readback(frozen.selection(), &mut reader, TreeLimits::default())
            .unwrap()
            .resume(owners, &coordination, identity, 4)
            .is_err()
    );
    assert!(!owners.obligations(100).unwrap().complete_claim_coverage());
    assert!(root.join("managed-native-output/master.mkv").is_file());
}
