use reel::{
    tig_publication::{ExistingPublisher, Profile},
    tig_scene_production::{OutputBinding, Publication, RenderedCandidate},
};
use reel_assets::{Config, PathPolicy};
use std::{fs, io::Read, path::Path};
use tig_assets::graph::recipe::produced::{BoundSubject, Claim};
use tig_core::Object;
use tig_store::{
    AckLevel, Capabilities, Error, Key, PublicationBackend, PublicationGuard, Request, State,
    existing::{ByteSource, Layout, Place, ReadCache, RequiredProtection},
};

struct Observed {
    inner: ExistingPublisher,
    visits: usize,
    requests: Vec<Request>,
    max_bytes: Option<u64>,
    fail_object: Option<Object>,
}
impl PublicationBackend for Observed {
    fn capabilities(&self) -> Capabilities {
        let mut result = self.inner.capabilities();
        if let Some(max) = self.max_bytes {
            result.max_object_bytes = max;
        }
        result
    }
    fn publication_guard(
        &mut self,
        object: &Object,
    ) -> tig_store::Result<Box<dyn PublicationGuard>> {
        self.visits += 1;
        self.inner.publication_guard(object)
    }
    fn protect(&mut self, request: &Request) -> tig_store::Result<State> {
        self.requests.push(request.clone());
        self.inner.protect(request)
    }
    fn verified_object(&mut self, object: &Object) -> tig_store::Result<bool> {
        self.inner.verified_object(object)
    }
    fn publish_object(
        &mut self,
        request: &Request,
        source: &mut dyn Read,
    ) -> tig_store::Result<()> {
        if self.fail_object.as_ref() == Some(&request.object) {
            return Err(Error::Io(
                std::io::ErrorKind::Interrupted,
                "fixture interruption".into(),
            ));
        }
        self.inner.publish_object(request, source)
    }
    fn commit(&mut self, request: &Request) -> tig_store::Result<()> {
        self.inner.commit(request)
    }
}

pub fn exercise(
    candidate: &RenderedCandidate,
    subject: &BoundSubject,
    root: &Path,
    bind_as: impl Fn(&str, &Object) -> BoundSubject,
) -> Claim {
    let cache = root.join("publication-cache");
    let coordination = root.join("publication-control");
    fs::create_dir_all(cache.join("objects/sha256")).unwrap();
    fs::create_dir(&coordination).unwrap();
    let make = || {
        let mut storage = Config {
            path_policy: PathPolicy::ConfiguredLocalRoots,
            cache_roots: vec![cache.clone()],
            coordination_root: Some(coordination.clone()),
            ..Default::default()
        };
        storage.capacity.minimum_free_bytes = 0;
        storage.capacity.minimum_free_basis_points = 0;
        ExistingPublisher::open(Profile {
            artifact_root: root.to_owned(),
            namespace: Key::parse("candidate-fixture").unwrap(),
            storage,
            destination: cache.clone(),
            reader_limits: Default::default(),
            protection: RequiredProtection::PreventMutation,
        })
        .unwrap()
    };
    let mut backend = Observed {
        inner: make(),
        visits: 0,
        requests: Vec::new(),
        max_bytes: None,
        fail_object: None,
    };
    let mut publication = Publication {
        operation: Key::parse("native-render-job").unwrap(),
        owner: Key::parse("native-producer").unwrap(),
        required: AckLevel::ProcessRestart,
    };
    let mapping = || OutputBinding {
        output_name: "master.mkv",
        subject,
    };
    assert!(candidate.publish(&[], &publication, &mut backend).is_err());
    assert!(
        candidate
            .publish(&[mapping(), mapping()], &publication, &mut backend)
            .is_err()
    );
    assert!(
        candidate
            .publish(
                &[OutputBinding {
                    output_name: "unknown",
                    subject
                }],
                &publication,
                &mut backend
            )
            .is_err()
    );
    for path in [
        root.join("rendered-native-output/master.mkv"),
        root.join("rendered-native-output/receipt.json"),
    ] {
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"changed after native verification").unwrap();
        assert!(
            candidate
                .publish(&[mapping()], &publication, &mut backend)
                .is_err()
        );
        fs::write(path, original).unwrap();
    }
    backend.max_bytes = Some(0);
    assert!(
        candidate
            .publish(&[mapping()], &publication, &mut backend)
            .is_err()
    );
    backend.max_bytes = None;
    publication.required = AckLevel::ProviderDurable;
    assert!(
        candidate
            .publish(&[mapping()], &publication, &mut backend)
            .is_err()
    );
    publication.required = AckLevel::ProcessRestart;
    assert_eq!(backend.visits, 0);
    assert!(!coordination.join("tig-publications").exists());

    // Selected output succeeds; receipt fails. Claim must not yet be published.
    backend.fail_object = Some(candidate.receipt_object().clone());
    assert!(
        candidate
            .publish(&[mapping()], &publication, &mut backend)
            .is_err()
    );
    assert_eq!(backend.requests.len(), 2);
    assert_eq!(&backend.requests[0].object, subject.subject().object());
    assert_eq!(&backend.requests[1].object, candidate.receipt_object());
    let expected = candidate.claim(&[mapping()]).unwrap();
    assert!(!reel_assets::object(&cache, expected.object().sha256.as_str()).exists());
    let partial = backend.requests.clone();
    drop(backend);

    let mut backend = Observed {
        inner: make(),
        visits: 0,
        requests: Vec::new(),
        max_bytes: None,
        fail_object: None,
    };
    let published = candidate
        .publish(&[mapping()], &publication, &mut backend)
        .unwrap();
    assert_eq!(published.claim().object(), expected.object());
    assert_eq!(published.receipt_object(), candidate.receipt_object());
    assert_eq!(published.acknowledgements().len(), 3);
    assert_eq!(backend.requests[..2], partial);
    for ack in published.acknowledgements() {
        assert_eq!(ack.level(), AckLevel::ProcessRestart);
    }
    assert_eq!(&backend.requests[2].object, expected.object());
    let first = backend.requests.clone();
    let repeat = candidate
        .publish(&[mapping()], &publication, &mut backend)
        .unwrap();
    assert_eq!(repeat.claim().object(), expected.object());
    assert_eq!(backend.requests[3..], first);
    // Two independent subjects share the master; a third selects a separate
    // audio output. Changing the set under an existing operation must conflict.
    let alias = bind_as("delivery/scene-alias", subject.subject().object());
    let mix_bytes = fs::read(root.join("rendered-native-output/mix.wav")).unwrap();
    let mix = bind_as("delivery/scene-mix", &Object::from_bytes(&mix_bytes));
    let mappings = || {
        [
            mapping(),
            OutputBinding {
                output_name: "master.mkv",
                subject: &alias,
            },
            OutputBinding {
                output_name: "mix.wav",
                subject: &mix,
            },
        ]
    };
    let conflict = candidate.publish(&mappings(), &publication, &mut backend);
    assert!(matches!(
        conflict.err().unwrap().downcast_ref::<Error>(),
        Some(Error::OperationConflict)
    ));
    publication.operation = Key::parse("multi-output-render-job").unwrap();
    let selected = candidate
        .publish(&mappings(), &publication, &mut backend)
        .unwrap();
    assert_eq!(selected.claim().outputs().len(), 3);
    assert_eq!(selected.acknowledgements().len(), 4);
    let requests: Vec<_> = selected
        .acknowledgements()
        .iter()
        .map(|ack| ack.request().clone())
        .collect();
    assert_eq!(&requests[2].object, candidate.receipt_object());
    assert_eq!(&requests[3].object, selected.claim().object());
    let mut reordered = mappings();
    reordered.reverse();
    let retry = candidate
        .publish(&reordered, &publication, &mut backend)
        .unwrap();
    assert_eq!(retry.claim().object(), selected.claim().object());
    assert_eq!(
        retry
            .acknowledgements()
            .iter()
            .map(|ack| ack.request().clone())
            .collect::<Vec<_>>(),
        requests
    );
    publication.owner = Key::parse("foreign-owner").unwrap();
    assert!(
        candidate
            .publish(&[mapping()], &publication, &mut backend)
            .is_err()
    );
    drop(backend);

    // Fresh provider reads the original objects; later original-context replay
    // uses the reloaded claim after Git/working metadata are moved offline.
    let mut cache_reader = ReadCache::new(tig_store::existing::Profile {
        places: vec![Place {
            root: cache.clone(),
            layout: Layout::ReelCas,
        }],
        required: RequiredProtection::PreventMutation,
        limits: Default::default(),
    })
    .unwrap();
    for object in [
        subject.subject().object(),
        published.receipt_object(),
        published.claim().object(),
    ] {
        let mut held = cache_reader.open_verified(object).unwrap();
        let mut bytes = Vec::new();
        held.read_to_end(&mut bytes).unwrap();
        assert_eq!(Object::from_bytes(&bytes), *object);
    }
    let mut bytes = Vec::new();
    cache_reader
        .open_verified(published.claim().object())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let restored = Claim::parse(expected.object(), &bytes).unwrap();
    restored.check_plan(candidate.recipe()).unwrap();
    assert_eq!(restored.bytes(), expected.bytes());
    assert!(candidate.claim(&[mapping()]).is_ok());
    restored
}
