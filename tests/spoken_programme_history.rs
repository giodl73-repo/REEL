#![cfg(feature = "tig-history")]
//! Controlled native fixtures only. No user cache, voices or approval evidence.
use reel::{
    scene_delivery::FileRef,
    spoken_programme::{
        Document,
        compiler::CompilationProfile,
        history::{self, Request},
    },
    tig_inputs::Source,
};
#[cfg(windows)]
use reel::{
    spoken_programme::history::Frozen,
    tig_inputs::ArchiveProfile,
    tig_publication::{ExistingPublisher, Profile as PublisherProfile},
    tig_scene_production::{DeliveryBudget, Publication},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tig_assets::graph;
#[cfg(windows)]
use tig_assets::{
    graph::recipe::produced::{self, BoundSubject, Claim, ContextPin, Selector},
    native::Instance,
};
use tig_core::{
    Object,
    tree::{MemoryNodes, TreeLimits},
};
use tig_store::existing::Layout;
#[cfg(windows)]
use tig_store::{
    Key,
    existing::{self, RequiredProtection},
    metadata::{MetadataReader, MetadataStore},
};

fn git_program() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join(if cfg!(windows) { "git.exe" } else { "git" }))
        .find(|p| p.is_file())
        .unwrap()
        .canonicalize()
        .unwrap()
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new(git_program())
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn reference(id: &str, object: &Object) -> FileRef {
    FileRef {
        path: id.into(),
        sha256: object.sha256.as_str().into(),
        bytes: object.bytes,
    }
}
#[cfg(windows)]
fn pin(file: &FileRef) -> Object {
    Object {
        sha256: tig_core::Digest::parse(&file.sha256).unwrap(),
        bytes: file.bytes,
    }
}
fn wave(k: usize, frames: u64) -> Vec<u8> {
    let size = (frames * 6) as u32;
    let mut out = b"RIFF".to_vec();
    out.extend((size + 36).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(48000u32.to_le_bytes());
    out.extend(288000u32.to_le_bytes());
    out.extend(6u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend(b"data");
    out.extend(size.to_le_bytes());
    for i in 0..frames {
        let n = if i < 2400 || i >= frames - 4800 {
            0
        } else {
            1000 * (k as i32 + 1) + (i % 97) as i32 - 48
        };
        out.extend_from_slice(&n.to_le_bytes()[..3]);
        out.extend_from_slice(&(-n).to_le_bytes()[..3]);
    }
    out
}

struct Fixture {
    temp: tempfile::TempDir,
    document: Document,
    bodies: BTreeMap<String, Vec<u8>>,
    definitions: BTreeMap<String, (Object, Vec<u8>)>,
    #[cfg(windows)]
    schema: Vec<u8>,
    profile: Value,
    request: Request,
}
impl Fixture {
    fn new(protected: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for name in [
            "records",
            "definitions",
            "cache",
            "cache/objects",
            "cache/objects/sha256",
            "artifact",
            "coordination",
        ] {
            fs::create_dir(root.join(name)).unwrap();
        }
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.name", "Native engineering fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join(".gitattributes"), "* text eol=lf\n").unwrap();
        let schema = serde_json::to_vec(&json!({"type":"object"})).unwrap();
        let schema_pin = Object::from_bytes(&schema);
        fs::write(root.join("definitions/authored.json"), &schema).unwrap();
        let mut definitions = BTreeMap::new();
        for id in ["index", "payload", "output"] {
            let bytes=serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":format!("types/{id}"),"authored_schema":schema_pin,
                "authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
            let pin = Object::from_bytes(&bytes);
            fs::write(root.join(format!("definitions/{id}.json")), &bytes).unwrap();
            definitions.insert(id.into(), (pin, bytes));
        }
        let mut document: Document =
            serde_json::from_str(include_str!("fixtures/spoken-programme/four-editions.json"))
                .unwrap();
        let text =
            b"Original engineering fixture text only; no production language approval.".to_vec();
        let reuse =
            b"Synthetic signals only; no voice consent, performance selection or release.".to_vec();
        let text_ref = reference("authority/text", &Object::from_bytes(&text));
        let reuse_ref = reference("authority/reuse", &Object::from_bytes(&reuse));
        for module in &mut document.modules {
            for cue in &mut module.cues {
                for script in &mut cue.scripts {
                    script.authority = text_ref.clone();
                }
            }
        }
        let mut bodies = BTreeMap::from([
            ("authority/text".into(), text),
            ("authority/reuse".into(), reuse),
        ]);
        for (k, p) in document.performances.iter_mut().enumerate() {
            let id = format!("takes/{}", p.id);
            let bytes = wave(k, p.duration_samples);
            p.source = reference(&id, &Object::from_bytes(&bytes));
            p.alignment.path = format!("align/{}", p.id).into();
            p.scope.evidence = reuse_ref.clone();
            p.observed_text_sha256 = Object::from_bytes(b"").sha256.as_str().into();
            bodies.insert(id, bytes);
        }
        let runtime = if protected {
            fs::read(std::env::current_exe().unwrap()).unwrap()
        } else {
            b"not-an-executed-runtime".to_vec()
        };
        let runtime_ref = reference("runtime/reel", &Object::from_bytes(&runtime));
        bodies.insert("runtime/reel".into(), runtime);
        let profiles = [
            ("index", graph::NodeKind::Container),
            ("payload", graph::NodeKind::Primary),
        ]
        .into_iter()
        .map(|(id, class)| graph::Profile {
            definition_id: format!("types/{id}"),
            definition: definitions[id].0.clone(),
            class,
            covered: std::collections::BTreeSet::from([graph::EdgeKind::Production]),
            rules: vec![],
            payloads: graph::Payloads::SelectedObject,
        })
        .collect();
        let request = Request {
            schema: history::SCHEMA.into(),
            declaration: reference("declaration", &Object::from_bytes(b"placeholder")),
            producer: runtime_ref,
            processing: None,
            profile: CompilationProfile::Engineering,
            profiles,
        };
        let profile = json!({"schema":reel::tig_inputs::PROFILE_SCHEMA,"git_executable":git_program(),"repository":root,"commit":"placeholder",
            "records":["records"],"definitions":["definitions"],"admitted":definitions.iter().map(|(id,(pin,_))|json!({"id":format!("types/{id}"),"object":pin,"payload":{"kind":"selected"}})).collect::<Vec<_>>(),
            "byte_source":{"places":[{"root":root.join("cache"),"layout":"reel-cas"}],"required":if protected {"prevent-mutation"}else{"cooperative-immutable"},
                "limits":{"object_bytes":128*1024*1024,"total_bytes":512*1024*1024,"requests":1024,"live_readers":128,"verify_millis":30000}}});
        let mut f = Self {
            temp,
            document,
            bodies,
            definitions,
            #[cfg(windows)]
            schema,
            profile,
            request,
        };
        f.refresh();
        f.commit();
        f
    }
    fn root(&self) -> &Path {
        self.temp.path()
    }
    fn seed_at(root: &Path, pin: &Object, bytes: &[u8]) {
        let path = Layout::ReelCas.object_path(root, &pin.sha256);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if path.exists() {
            assert_eq!(fs::read(path).unwrap(), bytes);
        } else {
            fs::write(path, bytes).unwrap();
        }
    }
    fn record(&self, id: &str, pin: &Object, kind: &str) -> Vec<u8> {
        let definition = &self.definitions[kind].0;
        serde_json::to_vec(&json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":format!("types/{kind}"),"sha256":definition.sha256,"bytes":definition.bytes},
            "authored":{},"streams":[],"selected":pin,"status":{},"metadata":{}})).unwrap()
    }
    fn refresh(&mut self) {
        for p in &mut self.document.performances {
            let bytes=serde_json::to_vec(&json!({"schema":reel::spoken_programme::compiler::ALIGNMENT_SCHEMA,"performance_id":p.id,"cue_id":p.cue_id,
                "language":p.language,"source":p.source,"sample_rate":48000,"duration_samples":p.duration_samples,"script_id":p.script_id,"script_sha256":p.script_sha256,
                "observation":{"kind":"engineering-signal"},"markers":p.markers,"word_bindings":[]})).unwrap();
            p.alignment.sha256 = Object::from_bytes(&bytes).sha256.as_str().into();
            p.alignment.bytes = bytes.len() as u64;
            self.bodies
                .insert(p.alignment.path.to_str().unwrap().into(), bytes);
        }
        let declaration = serde_json::to_vec(&self.document).unwrap();
        self.request.declaration = reference("declaration", &Object::from_bytes(&declaration));
        self.bodies.insert("declaration".into(), declaration);
        for (id, bytes) in &self.bodies {
            let pin = Object::from_bytes(bytes);
            Self::seed_at(&self.root().join("cache"), &pin, bytes);
            let kind = if id == "declaration" {
                "index"
            } else {
                "payload"
            };
            let name = Object::from_bytes(id.as_bytes()).sha256;
            fs::write(
                self.root().join(format!("records/{}.tig", name.as_str())),
                self.record(id, &pin, kind),
            )
            .unwrap();
        }
    }
    fn commit(&mut self) {
        git(
            self.root(),
            &["add", "records", "definitions", ".gitattributes"],
        );
        git(
            self.root(),
            &["commit", "-m", "bounded native signal versions"],
        );
        self.profile["commit"] = json!(git(self.root(), &["rev-parse", "HEAD"]));
    }
    fn source(&self) -> Source {
        Source::open(serde_json::from_value(self.profile.clone()).unwrap()).unwrap()
    }
    #[cfg(windows)]
    fn change_spanish_detail(&mut self) {
        let p = self
            .document
            .performances
            .iter_mut()
            .find(|p| p.id == "es-detail")
            .unwrap();
        let id = p.source.path.to_str().unwrap();
        self.bodies.get_mut(id).unwrap()[44 + 5000 * 6] ^= 1;
        p.source = reference(id, &Object::from_bytes(&self.bodies[id]));
        self.refresh();
        self.commit();
    }
    #[cfg(windows)]
    fn archive_profile(&self, frozen: &Frozen) -> ArchiveProfile {
        let context = frozen.context("es-technical").unwrap();
        serde_json::from_value(json!({"schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),
            "checkpoint":context.requirements().checkpoint(),"admitted":self.profile["admitted"],"byte_source":self.profile["byte_source"]})).unwrap()
    }
    #[cfg(windows)]
    fn metadata(&self) -> MetadataStore {
        MetadataStore::create(
            &self.root().join("metadata"),
            tig_store::metadata::Limits {
                nodes: 10000,
                bytes: 32 * 1024 * 1024,
                database_bytes: 64 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap()
    }
    #[cfg(windows)]
    fn publisher(&self) -> ExistingPublisher {
        let mut storage = reel_assets::Config {
            path_policy: reel_assets::PathPolicy::ConfiguredLocalRoots,
            cache_roots: vec![self.root().join("cache")],
            coordination_root: Some(self.root().join("coordination")),
            ..Default::default()
        };
        storage.capacity.minimum_free_bytes = 0;
        storage.capacity.minimum_free_basis_points = 0;
        storage.capacity.max_object_bytes = 128 * 1024 * 1024;
        ExistingPublisher::open(PublisherProfile {
            artifact_root: self.root().join("artifact"),
            namespace: Key::parse("spoken-fixture").unwrap(),
            storage,
            destination: self.root().join("cache"),
            reader_limits: existing::Limits::default(),
            protection: RequiredProtection::PreventMutation,
        })
        .unwrap()
    }
    #[cfg(windows)]
    fn subject(&self, edition: &str, wave: &Object) -> BoundSubject {
        let raw = self.record(&format!("outputs/{edition}"), wave, "output");
        let instance = Instance::parse(&raw).unwrap();
        let bound = instance
            .bind(&self.definitions["output"].1, &self.schema)
            .unwrap();
        BoundSubject::bind(
            &bound,
            &graph::Reference {
                id: "types/output".into(),
                object: self.definitions["output"].0.clone(),
            },
            Selector::Selected {},
        )
        .unwrap()
        .unwrap()
    }
}

#[test]
fn unsupported_protected_input_refuses() {
    let f = Fixture::new(false);
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let error = match history::freeze(&mut source, &f.request, &mut nodes, TreeLimits::default()) {
        Ok(_) => panic!("cooperative native path admitted"),
        Err(e) => e,
    };
    assert!(format!("{error:#}").contains("protected"));
    assert_eq!(source.byte_requests(), 0);
    assert_eq!(nodes.objects().count(), 0);
}

#[cfg(windows)]
#[test]
fn spanish_detail_change_preserves_other_consumed_recipes() {
    let mut f = Fixture::new(true);
    let mut nodes = MemoryNodes::default();
    let mut source = f.source();
    let old = history::freeze(&mut source, &f.request, &mut nodes, TreeLimits::default()).unwrap();
    drop(source);
    let original = f.root().join("original");
    let old_access = f.root().join("old-access");
    old.produce(&original).unwrap();
    old.write_access(&original, &old_access).unwrap();
    let old_checked = old.check_candidate(&original, &old_access).unwrap();
    let old_signal = old_checked.receipt().source_inputs["takes/es-detail"].clone();
    let mut reviews = BTreeMap::new();
    for edition in old_checked.receipt().recipes.keys() {
        let target = old_checked.review_target(edition).unwrap();
        let target_path = f.root().join(format!("target-{edition}.json"));
        fs::write(&target_path, &target).unwrap();
        let finding = f.root().join(format!("finding-{edition}.json"));
        fs::write(&finding,serde_json::to_vec(&json!({"schema":reel::review_decision::FINDING_SCHEMA,"record_id":format!("test-{edition}"),"reviewer_key":"synthetic-observer",
            "target_kind":"spoken-edition","kind":"selection","selected_option":"fixture-only","reason":"Synthetic binding test; not listening or approval.",
            "timestamp":"2026-11-21T00:00:00Z","scope":"engineering-signal","authority":"advisory","cites":[],
            "claims":{"authenticated":false,"signed":false,"consent":false,"approval":false}})).unwrap()).unwrap();
        let record_path = f.root().join(format!("review-{edition}.json"));
        reel::review_decision::write_record(target_path, finding, &record_path).unwrap();
        let record = serde_json::from_slice(&fs::read(record_path).unwrap()).unwrap();
        old_checked.check_review_binding(edition, &record).unwrap();
        reviews.insert(edition.clone(), record);
    }
    f.change_spanish_detail();
    let mut source = f.source();
    let new = history::freeze(&mut source, &f.request, &mut nodes, TreeLimits::default()).unwrap();
    drop(source);
    let next = f.root().join("next");
    let new_access = f.root().join("new-access");
    new.produce(&next).unwrap();
    new.write_access(&next, &new_access).unwrap();
    let new_checked = new.check_candidate(&next, &new_access).unwrap();
    assert_ne!(
        old.context("es-technical").unwrap().archive(),
        new.context("es-technical").unwrap().archive()
    );
    for (id, prior) in &old_checked.receipt().recipes {
        let after = &new_checked.receipt().recipes[id];
        let changed = id == "es-technical";
        assert_eq!(prior.compiled != after.compiled, changed);
        assert_eq!(prior.execution != after.execution, changed);
        assert_eq!(prior.native != after.native, changed);
        assert_eq!(
            new_checked.check_review_binding(id, &reviews[id]).is_err(),
            changed
        );
        if !changed {
            assert_eq!(
                old_checked.receipt().review_targets[id],
                new_checked.receipt().review_targets[id]
            );
        }
        let old_context = old.context(id).unwrap();
        let old_wave = pin(old_checked.wave(id).unwrap());
        // Current selection stays v1 until the project adopts a replacement.
        let subject = f.subject(id, &old_wave);
        let claim = Claim::create(
            old.recipe(id).unwrap(),
            ContextPin {
                root: old_context.selection().root.clone(),
                identity: old_context.selection().context.clone(),
            },
            vec![subject.subject().clone()],
        )
        .unwrap();
        let checked = claim.check_plan(old.recipe(id).unwrap()).unwrap();
        assert_eq!(
            produced::compare(new.recipe(id).unwrap(), Some(&subject), Some(&checked)).unwrap(),
            if changed {
                produced::Correspondence::StaleRecipe
            } else {
                produced::Correspondence::ClaimedRecipeMatch
            }
        );
    }
    assert!(
        Layout::ReelCas
            .object_path(&f.root().join("cache"), &old_signal.sha256)
            .is_file()
    );
}

#[cfg(windows)]
struct FailOnce {
    inner: ExistingPublisher,
    object: Object,
    failed: bool,
    requests: Vec<tig_store::Request>,
}
#[cfg(windows)]
impl tig_store::PublicationBackend for FailOnce {
    fn capabilities(&self) -> tig_store::Capabilities {
        self.inner.capabilities()
    }
    fn publication_guard(
        &mut self,
        object: &Object,
    ) -> tig_store::Result<Box<dyn tig_store::PublicationGuard>> {
        self.inner.publication_guard(object)
    }
    fn protect(&mut self, request: &tig_store::Request) -> tig_store::Result<tig_store::State> {
        self.requests.push(request.clone());
        self.inner.protect(request)
    }
    fn verified_object(&mut self, object: &Object) -> tig_store::Result<bool> {
        self.inner.verified_object(object)
    }
    fn publish_object(
        &mut self,
        request: &tig_store::Request,
        source: &mut dyn std::io::Read,
    ) -> tig_store::Result<()> {
        if !self.failed && request.object == self.object {
            self.failed = true;
            return Err(tig_store::Error::Io(
                std::io::ErrorKind::Interrupted,
                "synthetic prefix interruption".into(),
            ));
        }
        self.inner.publish_object(request, source)
    }
    fn commit(&mut self, request: &tig_store::Request) -> tig_store::Result<()> {
        self.inner.commit(request)
    }
}

#[cfg(windows)]
#[test]
fn retained_v1_reopens_and_stale_reviews_refuse() {
    let mut f = Fixture::new(true);
    let mut nodes = f.metadata();
    let mut source = f.source();
    let frozen =
        history::freeze(&mut source, &f.request, &mut nodes, TreeLimits::default()).unwrap();
    drop(source);
    let archive = f.archive_profile(&frozen);
    let candidate = f.root().join("candidate");
    let access = f.root().join("access");
    frozen.produce(&candidate).unwrap();
    frozen.write_access(&candidate, &access).unwrap();
    let checked = frozen.check_candidate(&candidate, &access).unwrap();
    let wave = pin(checked.wave("es-technical").unwrap());
    let review_target = Object::from_bytes(&checked.review_target("es-technical").unwrap());
    let mut review: reel::review_decision::ReviewDecisionRecord = serde_json::from_value(json!({
        "schema": reel::review_decision::DECISION_SCHEMA,
        "record_id": "retained-fixture", "source_finding_sha256": "0".repeat(64),
        "target_kind": "spoken-edition", "target_sha256": review_target.sha256.as_str(),
        "reviewer_key": "synthetic-observer", "kind": "selection",
        "selected_option": "fixture-only", "reason": "Binding test, not listening approval.",
        "timestamp": "2026-11-21T00:00:00Z", "scope": "engineering-signal",
        "authority": "advisory", "cited_record_sha256": [],
        "claims": {"authenticated": false, "signed": false, "consent": false, "approval": false},
        "tool_version": "fixture"
    }))
    .unwrap();
    checked
        .check_review_binding("es-technical", &review)
        .unwrap();
    let compiled = checked.receipt().recipes["es-technical"].compiled.clone();
    let subject = f.subject("es-technical", &wave);
    let publication = Publication {
        operation: Key::parse("retain-v1-es-technical").unwrap(),
        owner: Key::parse("fixture").unwrap(),
        required: tig_store::AckLevel::ProcessRestart,
    };
    let budget = DeliveryBudget {
        max_objects: 1024,
        max_bytes: 256 * 1024 * 1024,
    };
    let mut publisher = FailOnce {
        inner: f.publisher(),
        object: Object::from_bytes(checked.receipt_bytes()),
        failed: false,
        requests: vec![],
    };
    assert!(
        checked
            .publish_edition(
                "es-technical",
                &subject,
                &publication,
                &mut publisher,
                &budget
            )
            .is_err()
    );
    assert!(publisher.failed);
    let prefix = publisher.requests.clone();
    assert!(prefix.len() > 1);
    let published = checked
        .publish_edition(
            "es-technical",
            &subject,
            &publication,
            &mut publisher,
            &budget,
        )
        .unwrap();
    assert_eq!(&publisher.requests[prefix.len()..prefix.len() * 2], &prefix);
    let witness = nodes.finish().unwrap();
    drop(nodes);
    drop(publisher);
    drop(checked);
    drop(frozen);
    f.change_spanish_detail();
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root()
                .join(format!("offline-{}", name.trim_start_matches('.'))),
        )
        .unwrap();
    }
    assert!(Source::open(serde_json::from_value(f.profile.clone()).unwrap()).is_err());
    let mut reader = MetadataReader::open(&f.root().join("metadata"), &witness).unwrap();
    let reopened = history::reopen(
        published.receipt_object(),
        archive.clone(),
        "es-technical",
        published.claim(),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(reopened.native_scope_verified());
    reopened
        .check_review_binding("es-technical", &review)
        .unwrap();
    review.target_sha256 = "1".repeat(64);
    assert!(
        reopened
            .check_review_binding("es-technical", &review)
            .is_err()
    );
    assert!(reopened.summary().independent_pcm_check_passed);
    assert!(reopened.summary().access_checked);
    assert!(
        !reopened.summary().authority_verified
            && !reopened.summary().acoustic_word_fidelity_verified
    );
    assert_eq!(
        reopened.receipt().recipes["es-technical"].compiled,
        compiled
    );
    assert_eq!(reopened.receipt().candidate["edition-003.wav"], wave);
    assert_ne!(
        reopened.receipt().request.declaration.sha256,
        f.request.declaration.sha256
    );
    let missing = [
        reopened.receipt().source_inputs["runtime/reel"].clone(),
        reopened.receipt().source_inputs["takes/en-opening"].clone(),
    ];
    for (index, absent) in missing.iter().enumerate() {
        let alternate = f.root().join(format!("missing-{index}"));
        fs::create_dir(&alternate).unwrap();
        for ack in published.acknowledgements() {
            let object = &ack.request().object;
            if object.sha256 == absent.sha256 {
                continue;
            }
            let bytes =
                fs::read(Layout::ReelCas.object_path(&f.root().join("cache"), &object.sha256))
                    .unwrap();
            Fixture::seed_at(&alternate, object, &bytes);
        }
        let mut unavailable = archive.clone();
        unavailable.byte_source.places[0].root = alternate.clone();
        assert!(
            history::reopen(
                published.receipt_object(),
                unavailable,
                "es-technical",
                published.claim(),
                &mut reader,
                TreeLimits::default()
            )
            .is_err()
        );
        assert!(
            !Layout::ReelCas
                .object_path(&alternate, &absent.sha256)
                .exists()
        );
    }
}

#[cfg(windows)]
#[test]
fn publication_preflights_caps_and_rechecks_mutated_output_before_writes() {
    let f = Fixture::new(true);
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let frozen =
        history::freeze(&mut source, &f.request, &mut nodes, TreeLimits::default()).unwrap();
    let root = f.root().join("candidate");
    let access = f.root().join("access");
    frozen.produce(&root).unwrap();
    frozen.write_access(&root, &access).unwrap();
    let checked = frozen.check_candidate(&root, &access).unwrap();
    let wave = pin(checked.wave("en-executive").unwrap());
    let subject = f.subject("en-executive", &wave);
    let mut backend = f.publisher();
    let mut publication = Publication {
        operation: Key::parse("bounded-publish").unwrap(),
        owner: Key::parse("fixture").unwrap(),
        required: tig_store::AckLevel::ProcessRestart,
    };
    for budget in [
        DeliveryBudget {
            max_objects: 0,
            max_bytes: 256 * 1024 * 1024,
        },
        DeliveryBudget {
            max_objects: 1,
            max_bytes: 256 * 1024 * 1024,
        },
        DeliveryBudget {
            max_objects: 1024,
            max_bytes: 1,
        },
    ] {
        assert!(
            checked
                .publish_edition(
                    "en-executive",
                    &subject,
                    &publication,
                    &mut backend,
                    &budget
                )
                .is_err()
        );
    }
    let budget = DeliveryBudget {
        max_objects: 1024,
        max_bytes: 256 * 1024 * 1024,
    };
    publication.required = tig_store::AckLevel::ProviderDurable;
    assert!(
        checked
            .publish_edition(
                "en-executive",
                &subject,
                &publication,
                &mut backend,
                &budget
            )
            .is_err()
    );
    publication.required = tig_store::AckLevel::ProcessRestart;
    let path = root.join(&checked.wave("en-executive").unwrap().path);
    let mut bytes = fs::read(&path).unwrap();
    bytes[90000] ^= 1;
    fs::write(path, bytes).unwrap();
    assert!(
        checked
            .publish_edition(
                "en-executive",
                &subject,
                &publication,
                &mut backend,
                &budget
            )
            .is_err()
    );
    assert!(!f.root().join("coordination/tig-publications").exists());
    assert!(
        !f.root()
            .join("artifact/.local/cache-promotions.jsonl")
            .exists()
    );
}
