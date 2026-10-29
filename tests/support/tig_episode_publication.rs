//! Checked parent output publication, refusal and exact-prefix recovery.
use super::*;
use reel::tig_episode::{ProtectedComposition, ProtectedScenes, RenderedComposition};
use reel::tig_scene_production::DeliveryBudget;
use std::io::Read;
use tig_store::{Capabilities, Error, PublicationBackend, PublicationGuard, Request, State};

#[allow(clippy::too_many_arguments)]
pub(super) fn exercise(
    p: &Project,
    plan: &reel::tig_episode::CompositionJobPlan<'_>,
    foreign_plan: &reel::tig_episode::CompositionJobPlan<'_>,
    metadata: (&Path, &tig_store::metadata::Checkpoint),
    parent: &ProtectedComposition<'_, '_>,
    peer: &ProtectedComposition<'_, '_>,
    children: &ProtectedScenes<'_, '_>,
    journal: &mut SourceJournal,
    candidate: &RenderedComposition<'_>,
    output: &Path,
    scenes: &[Scene],
) {
    let mut record = read(p.repo.path().join("records/output-scene-a.tig"));
    record["id"] = json!("output/episode-es");
    record["selected"] = json!({"sha256":candidate.receipt().master_sha256,"bytes":candidate.receipt().master_bytes});
    let definition = fs::read(p.repo.path().join("definitions/output.json")).unwrap();
    let schema = fs::read(p.repo.path().join("definitions/authored.json")).unwrap();
    let bound = Instance::parse(&serde_json::to_vec(&record).unwrap())
        .unwrap()
        .bind(&definition, &schema)
        .unwrap();
    let subject = BoundSubject::bind(
        &bound,
        &Reference {
            id: "types/output".into(),
            object: Object::from_bytes(&definition),
        },
        Selector::Selected {},
    )
    .unwrap()
    .unwrap();
    let binding = || OutputBinding {
        output_name: "master.mkv",
        subject: &subject,
    };
    let mut backend = Observed {
        inner: p.publisher("delivered"),
        visits: 0,
        requests: Vec::new(),
        fail: None,
    };
    let mut publication = Publication {
        operation: key("complete-parent"),
        owner: key("episode-owner"),
        required: AckLevel::ProcessRestart,
    };
    let budget = DeliveryBudget {
        max_objects: 3,
        max_bytes: 8 * 1024 * 1024,
    };
    let source = p.source();
    let before = journal.checkpoint();
    let error = peer
        .publish_delivery(
            &source,
            children,
            journal,
            candidate,
            &[binding()],
            &publication,
            &mut backend,
            &budget,
        )
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("different protected job"),
        "{error:#}"
    );
    assert_eq!(
        backend.visits, 0,
        "an identical plan is not the same producing job"
    );
    for bindings in [
        Vec::new(),
        vec![binding(), binding()],
        vec![OutputBinding {
            output_name: "not-an-output",
            subject: &subject,
        }],
        vec![OutputBinding {
            output_name: "master.mkv",
            subject: &scenes[0].subject,
        }],
    ] {
        assert!(
            parent
                .publish_delivery(
                    &source,
                    children,
                    journal,
                    candidate,
                    &bindings,
                    &publication,
                    &mut backend,
                    &budget
                )
                .is_err()
        );
        assert_eq!(backend.visits, 0);
    }
    for denied in [
        DeliveryBudget {
            max_objects: 0,
            max_bytes: budget.max_bytes,
        },
        DeliveryBudget {
            max_objects: 2,
            max_bytes: budget.max_bytes,
        },
        DeliveryBudget {
            max_objects: 3,
            max_bytes: 1,
        },
    ] {
        assert!(
            parent
                .publish_delivery(
                    &source,
                    children,
                    journal,
                    candidate,
                    &[binding()],
                    &publication,
                    &mut backend,
                    &denied
                )
                .is_err()
        );
        assert_eq!(backend.visits, 0);
    }
    publication.required = AckLevel::ProviderDurable;
    assert!(
        parent
            .publish_delivery(
                &source,
                children,
                journal,
                candidate,
                &[binding()],
                &publication,
                &mut backend,
                &budget
            )
            .is_err()
    );
    assert_eq!(backend.visits, 0);
    publication.required = AckLevel::ProcessRestart;
    for name in ["master.mkv", "receipt.json"] {
        let path = output.join(name);
        let original = fs::read(&path).unwrap();
        let mut corrupt = original.clone();
        corrupt[0] ^= 1;
        fs::write(&path, corrupt).unwrap();
        assert!(
            parent
                .publish_delivery(
                    &source,
                    children,
                    journal,
                    candidate,
                    &[binding()],
                    &publication,
                    &mut backend,
                    &budget
                )
                .is_err()
        );
        assert_eq!(backend.visits, 0);
        fs::write(path, original).unwrap();
    }
    backend.fail = Some(candidate.receipt_object().clone());
    assert!(
        parent
            .publish_delivery(
                &source,
                children,
                journal,
                candidate,
                &[binding()],
                &publication,
                &mut backend,
                &budget
            )
            .is_err()
    );
    assert_eq!(backend.requests.len(), 2);
    let prefix = backend.requests.clone();
    backend.fail = None;
    let publication_result = parent
        .publish_delivery(
            &source,
            children,
            journal,
            candidate,
            &[binding()],
            &publication,
            &mut backend,
            &budget,
        )
        .unwrap();
    let published = publication_result.published();
    assert_eq!(published.acknowledgements().len(), 3);
    assert_eq!(
        published.delivery_outputs().unwrap()["master.mkv"],
        *subject.subject().object()
    );
    assert_eq!(published.receipt_object(), candidate.receipt_object());
    assert_eq!(
        published.claim().object(),
        candidate.claim(&[binding()]).unwrap().object()
    );
    assert_eq!(&backend.requests[2..4], &prefix);
    let replay = parent
        .publish_delivery(
            &source,
            children,
            journal,
            candidate,
            &[binding()],
            &publication,
            &mut backend,
            &budget,
        )
        .unwrap();
    assert_eq!(
        published.claim().object(),
        replay.published().claim().object()
    );
    let mut writer = MetadataStore::open(metadata.0, metadata.1).unwrap();
    assert!(
        foreign_plan
            .freeze_outputs(&publication_result, &mut writer, TreeLimits::default())
            .is_err()
    );
    let retained = plan
        .freeze_outputs(&publication_result, &mut writer, TreeLimits::default())
        .unwrap();
    let witness = writer.finish().unwrap();
    drop(writer);
    let offline = output.with_extension("unavailable");
    assert!(!offline.exists());
    fs::rename(output, &offline).unwrap();
    let readback = plan.readback_outputs(
        &publication_result,
        retained.selection(),
        &mut MetadataReader::open(metadata.0, &witness).unwrap(),
        TreeLimits::default(),
    );
    fs::rename(offline, output).unwrap();
    let readback = readback.unwrap();
    assert!(
        foreign_plan
            .readback_outputs(
                &publication_result,
                retained.selection(),
                &mut MetadataReader::open(metadata.0, &witness).unwrap(),
                TreeLimits::default()
            )
            .is_err()
    );
    use tig_git::source::native::job::Role;
    assert!(
        readback.of(Role::Primary).is_empty(),
        "compiled outputs are not primaries"
    );
    assert_eq!(
        readback.of(Role::ClaimedOutput),
        &BTreeSet::from([subject.subject().object().clone()])
    );
    for role in [Role::Composition, Role::Runtime, Role::Control] {
        assert_eq!(readback.of(role), parent.resources().of(role));
    }
    assert!(
        parent
            .resources()
            .of(Role::Evidence)
            .is_subset(readback.of(Role::Evidence))
    );
    assert!(
        readback
            .of(Role::Evidence)
            .contains(candidate.receipt_object())
    );
    assert!(
        !readback.complete_claim_coverage(),
        "separate child owners must not be retired"
    );
    assert!(
        plan.readback_outputs(
            &publication_result,
            retained.selection(),
            &mut tig_core::tree::MemoryNodes::default(),
            TreeLimits::default()
        )
        .is_err()
    );
    assert!(
        plan.readback_outputs(
            &publication_result,
            parent.resources().selection(),
            &mut MetadataReader::open(metadata.0, &witness).unwrap(),
            TreeLimits::default()
        )
        .is_err()
    );
    assert_eq!(
        journal.checkpoint(),
        before,
        "publication is not source retirement"
    );
    assert!(output.join("master.mkv").is_file());
    for request in backend.requests {
        assert!(
            tig_store::recover(&mut p.publisher("delivered"), &request)
                .unwrap()
                .is_some()
        );
    }
}
struct Observed {
    inner: tig_publication::ExistingPublisher,
    visits: usize,
    requests: Vec<Request>,
    fail: Option<Object>,
}
impl PublicationBackend for Observed {
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }
    fn publication_guard(&mut self, o: &Object) -> tig_store::Result<Box<dyn PublicationGuard>> {
        self.visits += 1;
        self.inner.publication_guard(o)
    }
    fn protect(&mut self, r: &Request) -> tig_store::Result<State> {
        self.requests.push(r.clone());
        self.inner.protect(r)
    }
    fn verified_object(&mut self, o: &Object) -> tig_store::Result<bool> {
        self.inner.verified_object(o)
    }
    fn publish_object(&mut self, r: &Request, s: &mut dyn Read) -> tig_store::Result<()> {
        if self.fail.as_ref() == Some(&r.object) {
            return Err(Error::Io(
                std::io::ErrorKind::Interrupted,
                "fixture parent interruption".into(),
            ));
        }
        self.inner.publish_object(r, s)
    }
    fn commit(&mut self, r: &Request) -> tig_store::Result<()> {
        self.inner.commit(r)
    }
}
