//! A complete tiny project. All stores, Git remotes and media are temporary.
use super::*;
use reel::{
    episode_conform, media_runtime, scene_delivery, tig_publication, tig_scene,
    tig_scene_production::{
        OutputBinding, Publication,
        jobs::{Coordination, Identity, JobPlan},
    },
};
use std::{
    collections::{BTreeSet, HashMap},
    io::Cursor,
};
use tig_assets::{
    graph::{
        Reference,
        recipe::{
            Plan,
            produced::{BoundSubject, Claim, Correspondence, Selector},
        },
    },
    native::Instance,
};
use tig_core::tree::TreeLimits;
use tig_store::{
    AckLevel, Key,
    existing::RequiredProtection,
    metadata::{MetadataReader, MetadataStore},
    source_claims::SourceJournal,
};

const UNITS: [&str; 4] = ["opening", "scene-a", "scene-b", "credits"];
fn key(value: &str) -> Key {
    Key::parse(value).unwrap()
}
fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn pin(root: &Path, path: &Path) -> Value {
    reference(
        path.strip_prefix(root).unwrap().to_str().unwrap(),
        &Object::from_bytes(&fs::read(path).unwrap()),
    )
}
fn object_file(cache: &Path, object: &Object) -> PathBuf {
    reel_assets::object(cache, object.sha256.as_str())
}

struct Project {
    repo: tempfile::TempDir,
    data: tempfile::TempDir,
    profile: Value,
    requests: Vec<tig_scene::Request>,
    programs: media_runtime::BoundPrograms,
    runtime: media_runtime::Bindings,
    owners: SourceJournal,
}
struct Scene {
    unit: String,
    job: PathBuf,
    output: PathBuf,
    recipe: Plan,
    claim: Claim,
    subject: BoundSubject,
    delivery_receipt: Object,
    primary: BTreeSet<Object>,
    controls: BTreeSet<Object>,
    metadata: PathBuf,
    metadata_checkpoint: tig_store::metadata::Checkpoint,
    archive: reel::tig_inputs::ArchiveProfile,
}
impl Project {
    fn hot(&self) -> PathBuf {
        self.data.path().join("hot")
    }
    fn state(&self) -> PathBuf {
        self.data.path().join("state")
    }
    fn publisher(&self, tier: &str) -> tig_publication::ExistingPublisher {
        let storage=serde_json::from_value(json!({"path_policy":"configured-local-roots","cache_roots":[self.data.path().join("hot"),self.data.path().join("history"),self.data.path().join("delivered")],
            "archive_roots":[self.data.path().join("archive")],"coordination_root":self.data.path().join("coordination"),"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0,"max_object_bytes":8388608}})).unwrap();
        tig_publication::ExistingPublisher::open(tig_publication::Profile {
            artifact_root: self.state(),
            namespace: key("fake-episode"),
            storage,
            destination: self.data.path().join(tier),
            reader_limits: Default::default(),
            protection: RequiredProtection::PreventMutation,
        })
        .unwrap()
    }
    fn publish(&self, tier: &str, bytes: &[u8]) -> Object {
        let object = Object::from_bytes(bytes);
        let request = tig_store::Request {
            operation: key(&format!("{tier}-{}", object.sha256.as_str())),
            owner: key("fixture"),
            object: object.clone(),
            required: AckLevel::ProcessRestart,
        };
        let mut backend = self.publisher(tier);
        tig_store::publish(&mut backend, &request, &mut Cursor::new(bytes)).unwrap();
        assert!(
            tig_store::recover(&mut self.publisher(tier), &request)
                .unwrap()
                .is_some()
        );
        object
    }
    fn source(&self) -> Source {
        Source::open(serde_json::from_value(self.profile.clone()).unwrap()).unwrap()
    }
    fn commit(&mut self, message: &str) -> String {
        git(self.repo.path(), &["add", "records", "definitions"]);
        git(self.repo.path(), &["commit", "-m", message]);
        let commit = git(self.repo.path(), &["rev-parse", "HEAD"]);
        self.profile["commit"] = json!(commit);
        commit
    }
    fn new() -> Self {
        let (bootstrap, request) = native_scene_fixture();
        let repo = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        for name in ["records", "definitions"] {
            fs::create_dir(repo.path().join(name)).unwrap();
            for entry in fs::read_dir(bootstrap.root().join(name)).unwrap() {
                let e = entry.unwrap();
                fs::copy(e.path(), repo.path().join(name).join(e.file_name())).unwrap();
            }
        }
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.name", "Fixture"]);
        git(
            repo.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(repo.path(), &["config", "core.autocrlf", "false"]);
        for name in [
            "hot/objects/sha256",
            "history/objects/sha256",
            "archive/objects/sha256",
            "delivered/objects/sha256",
            "coordination",
            "state/.local",
            "state/registry",
            "controls",
            "jobs",
        ] {
            fs::create_dir_all(data.path().join(name)).unwrap();
        }
        let locate = |name: &str| {
            std::env::split_paths(&std::env::var_os("PATH").unwrap())
                .map(|p| p.join(format!("{name}.exe")))
                .find(|p| p.is_file())
                .unwrap()
                .canonicalize()
                .unwrap()
        };
        let file = |path: PathBuf| {
            let observed = media_runtime::ByteIdentity::observe_file(&path).unwrap();
            reel::scene_delivery::FileRef {
                path,
                sha256: observed.sha256,
                bytes: observed.bytes,
            }
        };
        let runtime = media_runtime::Bindings {
            host: media_runtime::ByteIdentity::observe_file(&std::env::current_exe().unwrap())
                .unwrap(),
            ffmpeg: file(locate("ffmpeg")),
            ffprobe: file(locate("ffprobe")),
            dependencies: Default::default(),
        };
        let programs = media_runtime::BoundPrograms::bind(&runtime).unwrap();
        let owners = SourceJournal::create(
            &data.path().join("owners"),
            key("fake-episode"),
            key("epoch"),
        )
        .unwrap();
        let mut project = Self {
            repo,
            data,
            profile: bootstrap.profile.clone(),
            requests: Vec::new(),
            programs,
            runtime,
            owners,
        };
        project.profile["repository"] = json!(project.repo.path());
        project.profile["byte_source"] = json!({"places":[{"root":project.hot(),"layout":"reel-cas"},{"root":project.data.path().join("history"),"layout":"reel-cas"}],"required":"prevent-mutation"});
        for entry in fs::read_dir(bootstrap.root().join("cache/objects")).unwrap() {
            let e = entry.unwrap();
            project.publish("hot", &fs::read(e.path()).unwrap());
        }
        let controls = project.data.path().join("controls");
        let template = json!({"schema":"reel.episode-master-template.v1","template_id":"master","ordered_roles":["chapter-scenes"],"optional_roles":[]});
        let template_pin = write(&controls, "master.json", &template);
        let catalog = json!({"schema":"reel.scene-template-catalog.v1","templates":[{"template_id":"master","kind":"episode-master","definition_sha256":template_pin.sha256,"required_content_keys":[]}]});
        let episode = json!({"schema":"reel.episode-authoring.v1","episode_id":"episode-1","season_id":"season-1","authoring_state":"ready-for-private-build","master_template_id":"master","scene_policy_id":"policy","presentation":[],"score_palette":[],"scene_ids":UNITS});
        for (name, value) in [
            ("catalog.json", catalog.clone()),
            ("episode.json", episode.clone()),
            (
                "season.json",
                json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"season-1","assets":{}}),
            ),
            (
                "episode-bindings.json",
                json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"episode-1","assets":{}}),
            ),
        ] {
            write(&controls, name, &value);
        }
        let mut scene_definition = read(project.repo.path().join("definitions/scene.json"));
        scene_definition["authoring"]["properties"]
            .as_array_mut()
            .unwrap()
            .push(json!("episode_id"));
        let scene_definition_pin = write(
            project.repo.path(),
            "definitions/scene.json",
            &scene_definition,
        );
        for admission in project.profile["admitted"].as_array_mut().unwrap() {
            if admission["id"] == "types/scene" {
                admission["object"] = json!(scene_definition_pin);
            }
        }
        let mut base = read(project.repo.path().join("records/scene.tig"));
        base["definition"]["sha256"] = json!(scene_definition_pin.sha256);
        base["definition"]["bytes"] = json!(scene_definition_pin.bytes);
        base["authored"]["episode_id"] = json!("episode-1");
        write(project.repo.path(), "records/scene.tig", &base);
        let original: Object = serde_json::from_value(base["selected"].clone()).unwrap();
        let native = read(object_file(&project.hot(), &original));
        let media = read(project.repo.path().join("records/cel-a.tig"));
        for (unit, color) in [("opening", [255, 255, 0]), ("credits", [0, 255, 255])] {
            let mut cel = media.clone();
            cel["id"] = json!(format!("cel/{unit}"));
            cel["selected"] = json!(project.publish("hot", &picture(color)));
            write(
                project.repo.path(),
                &format!("records/cel-{unit}.tig"),
                &cel,
            );
        }
        let schema = Object::from_bytes(
            &fs::read(project.repo.path().join("definitions/authored.json")).unwrap(),
        );
        let output_def = write(
            project.repo.path(),
            "definitions/output.json",
            &json!({"schema":"tig.definition.design-1","id":"types/output","authored_schema":schema,"authoring":{"properties":["scene","language","component_kind","source_segment_id"],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        project.profile["admitted"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"types/output","object":output_def,"payload":{"kind":"selected"}}));
        write(
            project.repo.path(),
            "records/episode.tig",
            &json!({"schema":"tig.instance.design-1","id":"episode/fake","definition":{"id":"types/output","sha256":output_def.sha256,"bytes":output_def.bytes},"authored":{},"selected":null,"streams":[],"status":{},"metadata":{}}),
        );
        for unit in UNITS {
            let mut scene = native.clone();
            scene["scene_id"] = json!(unit);
            scene["languages"]["es"]["events"]
                .as_array_mut()
                .unwrap()
                .truncate(1);
            let mut record = base.clone();
            record["id"] = json!(format!("scene/{unit}"));
            record["selected"] =
                json!(project.publish("hot", &serde_json::to_vec(&scene).unwrap()));
            record["authored"]["scene_id"] = json!(unit);
            let cel = match unit {
                "scene-a" => "cel/a".to_owned(),
                "scene-b" => "cel/b".to_owned(),
                _ => format!("cel/{unit}"),
            };
            record["authored"]["binding_ids"] = json!({"a":{"id":cel},"take":{"id":"cue/main"},"alignment":{"id":"alignment/main"}});
            for stream in record["streams"].as_array_mut().unwrap() {
                match stream["slot"].as_str().unwrap() {
                    "catalog.json" => {
                        stream["object"] =
                            json!(project.publish("hot", &serde_json::to_vec(&catalog).unwrap()))
                    }
                    "episode.json" => {
                        stream["object"] =
                            json!(project.publish("hot", &serde_json::to_vec(&episode).unwrap()))
                    }
                    _ => {}
                }
            }
            write(
                project.repo.path(),
                &format!("records/scene-{unit}.tig"),
                &record,
            );
            let mut req: Value = serde_json::to_value(&request).unwrap();
            req["scene_id"] = json!(format!("scene/{unit}"));
            req["delivery_id"] = json!(unit);
            project.requests.push(serde_json::from_value(req).unwrap());
            write(
                project.repo.path(),
                &format!("records/output-{unit}.tig"),
                &json!({"schema":"tig.instance.design-1","id":format!("output/{unit}"),"definition":{"id":"types/output","sha256":output_def.sha256,"bytes":output_def.bytes},"authored":{"scene":{"id":format!("scene/{unit}")},"language":"es","component_kind":"scene","source_segment_id":unit},"selected":null,"streams":[],"status":{},"metadata":{}}),
            );
        }
        fs::remove_file(project.repo.path().join("records/scene.tig")).unwrap();
        project.commit("declare tiny episode and independently versioned inputs");
        project
    }
    fn render(&mut self, index: usize, version: &str) -> Scene {
        self.render_impl(index, version, false)
    }
    fn render_delivery(&mut self, index: usize, version: &str) -> Scene {
        self.render_impl(index, version, true)
    }
    fn render_impl(&mut self, index: usize, version: &str, complete: bool) -> Scene {
        let unit = UNITS[index];
        let request = &self.requests[index];
        let mut source = self.source();
        let producer = Object::from_bytes(self.programs.manifest_bytes());
        let recipe = tig_scene::recipe(&mut source, request, &producer).unwrap();
        let requirements = tig_scene::requirements(&mut source, request).unwrap();
        let primary = requirements.primary().clone();
        let controls = requirements.controls().clone();
        let root = self
            .data
            .path()
            .join("jobs")
            .join(format!("{unit}-{version}"));
        fs::create_dir(&root).unwrap();
        let metadata = root.join("metadata");
        let mut writer = MetadataStore::create(
            &metadata,
            tig_store::metadata::Limits {
                nodes: 8192,
                bytes: 8 * 1024 * 1024,
                database_bytes: 16 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let context =
            tig_scene::freeze_context(&mut source, request, &mut writer, TreeLimits::default())
                .unwrap();
        let original: Profile = serde_json::from_value(self.profile.clone()).unwrap();
        let archive = reel::tig_inputs::ArchiveProfile {
            schema: reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA.into(),
            archive: context.archive().clone(),
            checkpoint: source.checkpoint().clone(),
            limits: original.limits,
            admitted: original.admitted,
            byte_source: original.byte_source,
        };
        let plan = JobPlan::from_context(&mut source, request, &context, &self.programs).unwrap();
        let frozen = plan.freeze(&mut writer, TreeLimits::default()).unwrap();
        let checkpoint = writer.finish().unwrap();
        drop(writer);
        let mut reader = MetadataReader::open(&metadata, &checkpoint).unwrap();
        let mut job = plan
            .readback(frozen.selection(), &mut reader, TreeLimits::default())
            .unwrap()
            .protect(
                &mut self.owners,
                &Coordination {
                    root: self.data.path().join("coordination"),
                },
                Identity {
                    source: key(&format!("{unit}-{version}")),
                    owner: key("fixture"),
                },
                0,
                &key(&format!("start-{unit}-{version}")),
            )
            .unwrap();
        let job_dir = root.join("source");
        let output = root.join("render");
        let candidate = job
            .render(&mut self.owners, &mut source, &job_dir, &output)
            .unwrap();
        assert_eq!(
            candidate.candidate().recipe().fingerprint(),
            recipe.fingerprint()
        );
        let mut record = read(self.repo.path().join(format!("records/output-{unit}.tig")));
        let master = &candidate.candidate().receipt().outputs["master.mkv"];
        record["selected"] = json!({"sha256":master.sha256,"bytes":master.bytes});
        let definition = fs::read(self.repo.path().join("definitions/output.json")).unwrap();
        let schema = fs::read(self.repo.path().join("definitions/authored.json")).unwrap();
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
        let mut backend = self.publisher("hot");
        let bindings = [OutputBinding {
            output_name: "master.mkv",
            subject: &subject,
        }];
        let publication = Publication {
            operation: key(&format!("render-{unit}-{version}")),
            owner: key("fixture"),
            required: AckLevel::ProcessRestart,
        };
        let published = if complete {
            if index == 1 {
                super::candidate_publication::exercise_delivery(
                    candidate.candidate(),
                    &subject,
                    self.publisher("hot"),
                    &output,
                );
            }
            job.publish_delivery(
                &mut self.owners,
                &candidate,
                &bindings,
                &publication,
                &mut backend,
                &reel::tig_scene_production::DeliveryBudget {
                    max_objects: 64,
                    max_bytes: 8 * 1024 * 1024,
                },
            )
            .unwrap()
        } else {
            job.publish(
                &mut self.owners,
                &candidate,
                &bindings,
                &publication,
                &mut backend,
            )
            .unwrap()
        };
        let claim = Claim::parse(
            published.published().claim().object(),
            published.published().claim().bytes(),
        )
        .unwrap();
        claim.check_plan(&recipe).unwrap();
        let mut writer = MetadataStore::open(&metadata, &checkpoint).unwrap();
        let frozen_outputs = job
            .freeze_outputs(
                &mut self.owners,
                &published,
                &mut writer,
                TreeLimits::default(),
            )
            .unwrap();
        let completed = writer.finish().unwrap();
        drop(writer);
        let mut reader = MetadataReader::open(&metadata, &completed).unwrap();
        let ready = job
            .readback_outputs(
                &mut self.owners,
                &published,
                frozen_outputs.selection(),
                &mut reader,
                TreeLimits::default(),
            )
            .unwrap();
        assert_eq!(published.published().delivery_outputs().is_some(), complete);
        if let Some(outputs) = published.published().delivery_outputs() {
            for object in outputs.values() {
                assert!(
                    ready
                        .resources()
                        .of(tig_git::source::native::job::Role::Evidence)
                        .contains(object)
                );
                assert!(
                    !ready
                        .resources()
                        .of(tig_git::source::native::job::Role::Primary)
                        .contains(object)
                );
            }
        }
        job.adopt_outputs(
            &mut self.owners,
            &ready,
            &key(&format!("adopt-{unit}-{version}")),
        )
        .unwrap();
        self.owners
            .apply(
                key(&format!("finish-{unit}-{version}")),
                tig_store::source_claims::Command::Withdraw {
                    source: key(&format!("{unit}-{version}")),
                    owner: key("fixture"),
                    expected_revision: job.revision(),
                },
            )
            .unwrap();
        record["metadata"]["production"] =
            json!({"claim":claim.object(),"receipt":published.published().receipt_object()});
        write(
            self.repo.path(),
            &format!("records/output-{unit}.tig"),
            &record,
        );
        Scene {
            unit: unit.into(),
            job: job_dir.join("delivery/job.json"),
            output,
            recipe,
            claim,
            subject,
            delivery_receipt: published.published().receipt_object().clone(),
            primary,
            controls,
            metadata,
            metadata_checkpoint: completed,
            archive,
        }
    }
    fn assemble(&self, scenes: &[Scene], version: &str) -> (Object, PathBuf) {
        self.assemble_using(scenes, version, None)
    }
    fn assemble_using(
        &self,
        scenes: &[Scene],
        version: &str,
        outputs: Option<&BTreeMap<String, &dyn reel::scene_delivery::InputFiles>>,
    ) -> (Object, PathBuf) {
        let root = self.data.path();
        let controls = root.join("controls");
        let mut owners = Vec::new();
        for scene in scenes {
            let job: Job = serde_json::from_slice(&fs::read(&scene.job).unwrap()).unwrap();
            owners.push(self.source().prepare(&job).unwrap());
        }
        let provided: BTreeMap<String, &dyn reel::scene_delivery::InputFiles> = scenes
            .iter()
            .zip(&owners)
            .map(|(s, i)| (s.unit.clone(), i as &dyn reel::scene_delivery::InputFiles))
            .collect();
        let segments:Vec<_>=scenes.iter().map(|s| {
            let master = reference(s.output.join("master.mkv").strip_prefix(root).unwrap().to_str().unwrap(),s.subject.subject().object());
            let receipt = reference(s.output.join("receipt.json").strip_prefix(root).unwrap().to_str().unwrap(),&s.delivery_receipt);
            json!({"kind":"scene","id":s.unit,"master":master,"source_receipt":receipt,"delivery_job":pin(root,&s.job),"delivery_receipt":receipt,"semantic_delivery":pin(root,&s.job.parent().unwrap().join("semantic-delivery.json"))})
        }).collect();
        let manifest = json!({"schema":"reel.episode-conform.v1","episode_id":"episode-1","language":"es","output_sample_rate":48000,"catalog":pin(root,&controls.join("catalog.json")),"master_template_definition":pin(root,&controls.join("master.json")),"episode":pin(root,&controls.join("episode.json")),"season_bindings":pin(root,&controls.join("season.json")),"episode_bindings":pin(root,&controls.join("episode-bindings.json")),"segments":segments});
        let name = format!("episode-{version}.json");
        write(root, &name, &manifest);
        let output = root.join(format!("episode-{version}"));
        let missing = provided
            .iter()
            .skip(1)
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        assert!(
            episode_conform::build_with_scene_inputs(
                &root.join(&name),
                root,
                root,
                &output,
                &missing
            )
            .is_err()
        );
        assert!(!output.exists());
        let receipt = if let Some(outputs) = outputs {
            let missing_outputs = outputs
                .iter()
                .skip(1)
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            assert!(
                episode_conform::build_with_scene_files(
                    &root.join(&name),
                    root,
                    root,
                    &output,
                    &provided,
                    &missing_outputs
                )
                .is_err()
            );
            assert!(!output.exists());
            episode_conform::build_with_segment_files(
                &root.join(name),
                root,
                root,
                &output,
                &provided,
                outputs,
                &BTreeMap::new(),
            )
            .unwrap()
        } else {
            episode_conform::build_with_scene_inputs(
                &root.join(name),
                root,
                root,
                &output,
                &provided,
            )
            .unwrap()
        };
        assert!(receipt.decoded_master_matches_ordered_segments);
        assert!(receipt.timestamps_verified);
        assert_eq!(receipt.total_frames, 96);
        assert_eq!(receipt.total_samples, 192000);
        let path = output.join("master.mkv");
        (self.publish("hot", &fs::read(&path).unwrap()), path)
    }
    fn deliver(&self, master: &Object, path: &Path) -> Object {
        assert_eq!(self.publish("delivered", &fs::read(path).unwrap()), *master);
        let mut record = read(self.repo.path().join("records/episode.tig"));
        record["selected"] = json!(master);
        record["metadata"]["delivery"] =
            json!({"target":"simulated-local-review","published":master,"creative_approval":false});
        write(self.repo.path(), "records/episode.tig", &record);
        self.publish(
            "hot",
            &fs::read(self.repo.path().join("records/episode.tig")).unwrap(),
        )
    }
}

fn storage_transition(
    p: &Project,
    old_inputs: &BTreeSet<Object>,
    new_inputs: &BTreeSet<Object>,
    old: &Object,
    new: &Object,
    manifests: [&Object; 2],
) {
    use reel_assets::{accounting, demotion, tiering, version_diff, versions};
    let root = p.state().canonicalize().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old_set: BTreeSet<_> = old_inputs
        .iter()
        .cloned()
        .chain([old.clone(), manifests[0].clone()])
        .collect();
    let new_set: BTreeSet<_> = new_inputs
        .iter()
        .cloned()
        .chain([new.clone(), manifests[1].clone()])
        .collect();
    let sizes: HashMap<_, _> = old_set
        .union(&new_set)
        .map(|o| (o.sha256.as_str().to_owned(), o.bytes))
        .collect();
    for object in &old_set {
        p.publish("history", &fs::read(object_file(&p.hot(), object)).unwrap());
    }
    write(
        &root,
        ".local/storage-rust.json",
        &json!({"path_policy":"configured-local-roots","cache_roots":[p.hot(),p.data.path().join("history")],"archive_roots":[p.data.path().join("archive")],"coordination_root":p.data.path().join("coordination"),"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}),
    );
    write(
        &root,
        "asset-project.json",
        &json!({"schema":"reel.asset-project.v1","project_id":"fake-episode","storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"max_inventory_objects":4096,"lfs_namespaces":[]}),
    );
    write(&root, "catalog.json", &json!({"videos":[]}));
    let rows = sizes
        .iter()
        .map(|(hash, size)| format!("{hash}\t{size}\n"))
        .collect::<String>();
    fs::write(root.join("registry/objects.tsv"), rows).unwrap();
    // This closed fixture has one delivered root. Input closures come from the
    // actual native graphs; generated scene outputs are not primary inputs.
    let mut state = versions::State {
        coverage_complete: true,
        ..Default::default()
    };
    for (name, objects, master, manifest, previous) in [
        ("v1", old_inputs, old, manifests[0], None),
        ("v2", new_inputs, new, manifests[1], Some("v1")),
    ] {
        let primary_inputs = objects
            .iter()
            .map(|o| o.sha256.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let outputs = BTreeSet::from([master.sha256.as_str().to_owned()]);
        assert!(primary_inputs.is_disjoint(&outputs));
        state
            .register(versions::Version {
                logical_id: "episode/fake".into(),
                version_id: name.into(),
                manifest_sha256: manifest.sha256.as_str().into(),
                payloads: primary_inputs.union(&outputs).cloned().collect(),
                dependencies: vec![],
                supersedes: previous.map(str::to_owned),
                deprecated: false,
                dependency_contract: Some(versions::DependencyContract {
                    primary_inputs,
                    outputs,
                    references: BTreeSet::new(),
                }),
            })
            .unwrap();
    }
    state.select("episode/fake", "v2").unwrap();
    write(
        &root,
        "registry/versions.json",
        &serde_json::to_value(&state).unwrap(),
    );
    let diff = version_diff::compare(
        &state,
        &sizes,
        ("episode/fake", "v1"),
        ("episode/fake", "v2"),
    )
    .unwrap();
    let candidates: BTreeSet<String> =
        serde_json::from_value(diff["eviction_candidate_sha256s"].clone()).unwrap();
    let expected: BTreeSet<String> = old_set
        .difference(&new_set)
        .map(|o| o.sha256.as_str().into())
        .collect();
    assert_eq!(candidates, expected);
    // Old cel, compiled episode, and its small immutable delivery record.
    assert_eq!(candidates.len(), 3);
    let census = accounting::report(&root).unwrap();
    let raw = serde_json::to_vec(&census).unwrap();
    fs::write(root.join("baseline.json"), &raw).unwrap();
    let policy = json!({"schema":"reel.asset-tier-policy.v1","project_id":"fake-episode","baseline":Object::from_bytes(&raw),"tiers":[{"id":"current","storage":"active","index":0},{"id":"previous","storage":"active","index":1},{"id":"archive","storage":"archive","index":0}],"selected_tier":"current","history_tier":"previous","cold_tier":"archive","history_versions":[{"logical_id":"episode/fake","version_id":"v1","manifest_sha256":manifests[0].sha256}],"tier_pins":{},"max_objects":4096,"max_plan_seconds":60,"expires_after_seconds":600,"max_batch_bytes":8388608,"max_batch_objects":100,"allow_same_volume_copies":true});
    write(&root, "policy.json", &policy);
    let plan = tiering::plan(
        &root,
        &root.join("policy.json"),
        &root.join("baseline.json"),
        now,
    )
    .unwrap();
    tiering::write_plan(&root.join("plan.json"), &plan).unwrap();
    write(
        &root,
        "scope.json",
        &json!({"schema":"reel.asset-demotion-request.v1","policy":root.join("policy.json"),"plan":root.join("plan.json"),"plan_sha256":plan["plan_sha256"],"consumer_roots":[root],"consumer_scope":"all-managed-cache-consumers","sha256s":candidates,"max_verify_bytes":16777216,"max_seconds":60,"allow_same_volume_copies":true}),
    );
    version_diff::eviction_request(
        &root,
        ("episode/fake", "v1"),
        ("episode/fake", "v2"),
        &root.join("scope.json"),
        &root.join("request.json"),
        now,
    )
    .unwrap();
    demotion::run(
        &root,
        &root.join("request.json"),
        &root.join(".local/dry"),
        false,
        now,
    )
    .unwrap();
    for hash in &expected {
        assert!(reel_assets::object(&p.hot(), hash).exists());
    }
    // Commits are per object, not batch-atomic. Force a late failure so resume
    // must preserve already verified removals and never remove the missing copy.
    let ordered = read(root.join("request.json"));
    let held_hash = ordered["sha256s"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    let previous = reel_assets::object(&p.data.path().join("history"), held_hash);
    let missing = p.data.path().join("missing-backup.blob");
    fs::rename(&previous, &missing).unwrap();
    assert!(
        demotion::run(
            &root,
            &root.join("request.json"),
            &root.join(".local/missing"),
            true,
            now
        )
        .is_err()
    );
    assert!(reel_assets::object(&p.hot(), held_hash).exists());
    for object in old_set.difference(&new_set) {
        if object.sha256.as_str() != held_hash {
            assert!(!object_file(&p.hot(), object).exists());
            assert_eq!(
                Object::from_bytes(
                    &fs::read(object_file(&p.data.path().join("history"), object)).unwrap()
                ),
                *object
            );
        }
    }
    fs::rename(missing, &previous).unwrap();
    demotion::run(
        &root,
        &root.join("request.json"),
        &root.join(".local/missing"),
        true,
        now,
    )
    .unwrap();
    for object in old_set.difference(&new_set) {
        assert!(!object_file(&p.hot(), object).exists());
        assert_eq!(
            Object::from_bytes(
                &fs::read(object_file(&p.data.path().join("history"), object)).unwrap()
            ),
            *object
        );
    }
    for object in &new_set {
        assert_eq!(
            Object::from_bytes(&fs::read(object_file(&p.hot(), object)).unwrap()),
            *object
        );
    }
}

#[test]
#[ignore = "requires native FFmpeg; complete cached delivery with no render directory"]
fn cached_scene_bundle_rechecks_without_render_directory_and_rejects_bad_bytes() {
    use std::os::windows::process::CommandExt;
    if let Some(case) = std::env::var_os("REEL_TEST_EPISODE_NO_PATH") {
        consume_episode_without_path(Path::new(&case));
        return;
    }
    use reel::{scene_delivery, tig_scene_outputs::Bundle};
    use tig_core::Digest;
    let mut p = Project::new();
    let scene = p.render_delivery(1, "cached-v1");
    let original_bytes = fs::read(scene.output.join("receipt.json")).unwrap();
    let original: scene_delivery::Receipt = serde_json::from_slice(&original_bytes).unwrap();
    let receipt = scene.delivery_receipt.clone();
    assert_eq!(Object::from_bytes(&original_bytes), receipt);
    let profile: tig_store::existing::Profile =
        serde_json::from_value(p.profile["byte_source"].clone()).unwrap();
    let job: Job = serde_json::from_slice(&fs::read(&scene.job).unwrap()).unwrap();
    let primary = p.source().prepare(&job).unwrap();
    // Preserve scratch for cleanup, but remove its old path from all lookups.
    let hidden = scene.output.with_extension("not-visible");
    fs::rename(&scene.output, &hidden).unwrap();
    assert!(!scene.output.exists());
    let bundle = Bundle::open(profile.clone(), &receipt).unwrap();
    let checked = scene_delivery::check_with_output_files(
        &scene.job,
        &primary,
        &bundle,
        bundle.receipt_ref(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&checked).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    let closure: BTreeSet<_> = bundle.objects().cloned().collect();
    assert!(closure.contains(&receipt));
    for file in original.outputs.values() {
        assert!(closure.contains(&Object {
            sha256: Digest::parse(&file.sha256).unwrap(),
            bytes: file.bytes
        }));
    }
    let mut stale = checked.outputs["master.mkv"].clone();
    stale.bytes += 1;
    assert!(bundle.verified_path(&stale).is_err());
    stale.path = "unbound.mkv".into();
    assert!(bundle.verified_path(&stale).is_err());
    let master = &checked.outputs["master.mkv"];
    let mut aliased = master.clone();
    aliased.path = "./master.mkv".into();
    assert!(bundle.verified_path(&aliased).is_err());
    let master_object = Object {
        sha256: Digest::parse(&master.sha256).unwrap(),
        bytes: master.bytes,
    };
    let master_path = object_file(&p.hot(), &master_object);
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(&master_path)
            .is_err()
    );
    drop(bundle);
    let saved = fs::read(&master_path).unwrap();
    let missing = master_path.with_extension("missing");
    fs::rename(&master_path, &missing).unwrap();
    assert!(Bundle::open(profile.clone(), &receipt).is_err());
    let mut too_small = profile.clone();
    too_small.limits.live_readers = 1;
    assert!(
        Bundle::open(too_small, &receipt)
            .err()
            .unwrap()
            .to_string()
            .contains("reader budget")
    );
    fs::rename(&missing, &master_path).unwrap();
    let mut corrupt = saved.clone();
    corrupt[0] ^= 1;
    fs::write(&master_path, corrupt).unwrap();
    assert!(Bundle::open(profile.clone(), &receipt).is_err());
    fs::write(&master_path, saved).unwrap();
    assert!(Bundle::open(profile.clone(), &receipt).is_ok());
    let mut cooperative = profile.clone();
    cooperative.required = RequiredProtection::CooperativeImmutable;
    assert!(Bundle::open(cooperative, &receipt).is_err());
    let mut bad = serde_json::to_value(&original).unwrap();
    bad["outputs"]["master.mkv"]["path"] = json!("../escape.mkv");
    let bad_receipt = p.publish("hot", &serde_json::to_vec(&bad).unwrap());
    assert!(Bundle::open(profile.clone(), &bad_receipt).is_err());
    let mut traversal = serde_json::to_value(&original).unwrap();
    let mut item = traversal["outputs"]
        .as_object_mut()
        .unwrap()
        .remove("master.mkv")
        .unwrap();
    item["path"] = json!("../escape.mkv");
    traversal["outputs"]["../escape.mkv"] = item;
    let traversal_receipt = p.publish("hot", &serde_json::to_vec(&traversal).unwrap());
    assert!(Bundle::open(profile.clone(), &traversal_receipt).is_err());
    let mut windows_names = serde_json::to_value(&original).unwrap();
    let mut alias = windows_names["outputs"]["picture.mkv"].clone();
    alias["path"] = json!("nested\\picture.mkv");
    windows_names["outputs"]["nested/picture.mkv"] = alias;
    let windows_receipt = p.publish("hot", &serde_json::to_vec(&windows_names).unwrap());
    let legacy = Bundle::open(profile.clone(), &windows_receipt).unwrap();
    let mut expected = original.outputs["picture.mkv"].clone();
    expected.path = "nested/picture.mkv".into();
    let canonical_path = legacy.verified_path(&expected).unwrap();
    expected.path = "nested\\picture.mkv".into();
    assert_eq!(legacy.verified_path(&expected).unwrap(), canonical_path);
    expected.path = "nested/./picture.mkv".into();
    assert!(legacy.verified_path(&expected).is_err());
    drop(legacy);
    let text = std::str::from_utf8(&original_bytes).unwrap();
    let duplicated = format!(
        "{{\"schema\":\"reel.scene-delivery-receipt.v0.1\",{}",
        &text[1..]
    );
    let duplicate_receipt = p.publish("hot", duplicated.as_bytes());
    assert!(Bundle::open(profile.clone(), &duplicate_receipt).is_err());
    let oversized = Object {
        sha256: receipt.sha256.clone(),
        bytes: 4 * 1024 * 1024 + 1,
    };
    assert!(Bundle::open(profile.clone(), &oversized).is_err());
    // A complete delivery can span tiers without copying it back into hot.
    let picture_file = &original.outputs["picture.mkv"];
    let picture_object = Object {
        sha256: Digest::parse(&picture_file.sha256).unwrap(),
        bytes: picture_file.bytes,
    };
    let hot_picture = object_file(&p.hot(), &picture_object);
    assert_eq!(
        p.publish("history", &fs::read(&hot_picture).unwrap()),
        picture_object
    );
    fs::remove_file(&hot_picture).unwrap();
    let distributed = Bundle::open(profile.clone(), &receipt).unwrap();
    assert!(
        distributed
            .verified_path(picture_file)
            .unwrap()
            .starts_with(p.data.path().join("history"))
    );
    assert!(!hot_picture.exists());
    drop(distributed);
    // Complete episode consumption: every original render directory is absent.
    let scenes = vec![
        p.render_delivery(0, "cached-v1"),
        scene,
        p.render_delivery(2, "cached-v1"),
        p.render_delivery(3, "cached-v1"),
    ];
    let bundles: Vec<_> = scenes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let selected = if i == 1 {
                receipt.clone()
            } else {
                fs::rename(&s.output, s.output.with_extension("not-visible")).unwrap();
                s.delivery_receipt.clone()
            };
            assert!(!s.output.exists());
            Bundle::open(profile.clone(), &selected).unwrap()
        })
        .collect();
    let providers = scenes
        .iter()
        .zip(&bundles)
        .map(|(s, b)| (s.unit.clone(), b as &dyn InputFiles))
        .collect();
    p.assemble_using(&scenes, "cached-episode", Some(&providers));
    let case = p.data.path().join("episode-no-path.json");
    write(
        p.data.path(),
        "episode-no-path.json",
        &json!({
            "source":p.profile,"runtime":p.runtime,"root":p.data.path(),
            "manifest":p.data.path().join("episode-cached-episode.json"),
            "jobs":scenes.iter().map(|s| (s.unit.clone(), s.job.clone())).collect::<BTreeMap<_,_>>(),
            "receipts":scenes.iter().map(|s| (s.unit.clone(), s.delivery_receipt.clone())).collect::<BTreeMap<_,_>>(),
        }),
    );
    let child = Command::new(std::env::current_exe().unwrap())
        .creation_flags(0x0800_0000)
        .args(["--ignored", "--exact", "fake_episode_lifecycle::cached_scene_bundle_rechecks_without_render_directory_and_rejects_bad_bytes", "--nocapture"])
        .env("PATH", "").env("REEL_TEST_EPISODE_NO_PATH", case)
        .output().unwrap();
    assert!(
        child.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(
        !hot_picture.exists(),
        "assembly must not hydrate history outputs into hot"
    );
}

fn consume_episode_without_path(case_path: &Path) {
    assert_eq!(std::env::var_os("PATH").unwrap(), "");
    let case = read(case_path);
    let profile: Profile = serde_json::from_value(case["source"].clone()).unwrap();
    let runtime: media_runtime::Bindings = serde_json::from_value(case["runtime"].clone()).unwrap();
    let programs = media_runtime::BoundPrograms::bind(&runtime).unwrap();
    let jobs: BTreeMap<String, PathBuf> = serde_json::from_value(case["jobs"].clone()).unwrap();
    let receipts: BTreeMap<String, Object> =
        serde_json::from_value(case["receipts"].clone()).unwrap();
    let mut source = Source::open(profile.clone()).unwrap();
    let primary: BTreeMap<_, _> = jobs
        .iter()
        .map(|(id, path)| {
            let job: Job = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            (id.clone(), source.prepare(&job).unwrap())
        })
        .collect();
    let inputs: BTreeMap<_, _> = primary
        .iter()
        .map(|(id, files)| (id.clone(), programs.with_inputs(files)))
        .collect();
    let inputs: BTreeMap<_, _> = inputs
        .iter()
        .map(|(id, files)| (id.clone(), files as &dyn InputFiles))
        .collect();
    let bundles: BTreeMap<_, _> = receipts
        .iter()
        .map(|(id, object)| {
            (
                id.clone(),
                reel::tig_scene_outputs::Bundle::open(profile.byte_source.clone(), object).unwrap(),
            )
        })
        .collect();
    let outputs = bundles
        .iter()
        .map(|(id, files)| (id.clone(), files as &dyn InputFiles))
        .collect();
    let root: PathBuf = serde_json::from_value(case["root"].clone()).unwrap();
    let manifest: PathBuf = serde_json::from_value(case["manifest"].clone()).unwrap();
    let base = PathBuf::from(".");
    let tools = programs.with_inputs(&base);
    struct Deny<'a>(&'a dyn InputFiles, scene_delivery::MediaProgram);
    impl InputFiles for Deny<'_> {
        fn verified_path(&self, item: &FileRef) -> anyhow::Result<PathBuf> {
            self.0.verified_path(item)
        }
        fn media_program(&self, program: scene_delivery::MediaProgram) -> anyhow::Result<PathBuf> {
            anyhow::ensure!(program != self.1, "episode tool denied");
            self.0.media_program(program)
        }
    }
    let presentations = BTreeMap::new();
    for denied in [
        scene_delivery::MediaProgram::Ffmpeg,
        scene_delivery::MediaProgram::Ffprobe,
    ] {
        let refused = root.join(format!("episode-refused-{denied:?}"));
        let error = episode_conform::build_with_files(
            &manifest,
            &root,
            &root,
            &refused,
            episode_conform::Files {
                scene_inputs: &inputs,
                scene_outputs: &outputs,
                presentations: &presentations,
                programs: &Deny(&tools, denied),
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("episode tool denied"),
            "{error:#}"
        );
        assert!(!refused.exists());
    }
    let receipt = episode_conform::build_with_files(
        &manifest,
        &root,
        &root,
        &root.join("episode-no-path"),
        episode_conform::Files {
            scene_inputs: &inputs,
            scene_outputs: &outputs,
            presentations: &presentations,
            programs: &tools,
        },
    )
    .unwrap();
    assert!(receipt.decoded_master_matches_ordered_segments);
    assert!(receipt.timestamps_verified);
    assert_eq!(receipt.total_frames, 96);
    assert_eq!(receipt.total_samples, 192000);
    // Exercise the non-copy paths too: declared geometry/color normalization,
    // lossless H264 intermediates, concat clocks and compact delivery validation.
    let mut compact = read(&manifest);
    compact["output_geometry"] = json!({"width":32,"height":32,"policy":"preserve-aspect-lanczos","assume_square_for_unspecified_sar":true,"assume_color_space_for_unspecified":"bt470bg"});
    compact["audio_frame_conform"] = json!("pad-silence-to-picture-boundaries");
    compact["compact_delivery"] = json!({"crf":18,"audio_bitrate_kbps":128,"retain_lossless_master":true,"intermediate_video_encoding":"h264-lossless"});
    let compact_path = root.join("episode-no-path-compact.json");
    write(&root, "episode-no-path-compact.json", &compact);
    let compact = episode_conform::build_with_files(
        &compact_path,
        &root,
        &root,
        &root.join("episode-no-path-compact"),
        episode_conform::Files {
            scene_inputs: &inputs,
            scene_outputs: &outputs,
            presentations: &presentations,
            programs: &tools,
        },
    )
    .unwrap();
    assert!(compact.decoded_master_matches_ordered_segments);
    assert!(compact.timestamps_verified);
    assert_eq!(compact.total_frames, 96);
    assert_eq!(compact.total_samples, 192000);
    let encoded = compact.encoded_delivery.unwrap();
    assert!(encoded.full_decode_verified);
    assert!(encoded.lossless_master_retained);
    assert_eq!(encoded.decoded_frames, 96);
}

fn scene_correspondence(
    p: &Project,
    scene: &Scene,
    index: usize,
    output_id: &str,
) -> anyhow::Result<reel::tig_episode::CurrentScene> {
    use reel::tig_episode::check_current_scene;
    let mut original_nodes =
        MetadataReader::open(&scene.metadata, &scene.metadata_checkpoint).unwrap();
    let producer = Object::from_bytes(p.programs.manifest_bytes());
    let (_, replay) = tig_scene::restore_claim(
        scene.archive.clone(),
        &p.requests[index],
        &producer,
        &scene.claim,
        &mut original_nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let mut source = p.source();
    let mut nodes = tig_core::tree::MemoryNodes::default();
    let context = tig_scene::freeze_context(
        &mut source,
        &p.requests[index],
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let plan =
        JobPlan::from_context(&mut source, &p.requests[index], &context, &p.programs).unwrap();
    let checked = check_current_scene(&source, output_id, &plan, &replay);
    assert_eq!(
        source.byte_requests(),
        0,
        "metadata correspondence must not open primary/output bodies"
    );
    checked
}

fn exercise_scene_correspondence(p: &mut Project, scenes: &[Scene]) {
    let original_profile = p.profile.clone();
    let scene = &scenes[2];
    let check = |p: &Project, output_id: &str| scene_correspondence(p, scene, 2, output_id);
    let checked = check(p, "output/scene-b").unwrap();
    assert_eq!(checked.selected(), scene.subject.subject().object());
    assert_eq!(checked.claim(), scene.claim.object());
    assert_eq!(checked.recipe(), scene.recipe.fingerprint());
    assert!(check(p, "output/scene-a").is_err());
    git(p.repo.path(), &["switch", "-c", "correspondence-cases"]);
    let request = add_episode_assembly(p);
    let mut proofs = scenes
        .iter()
        .enumerate()
        .map(|(index, scene)| {
            (
                scene.unit.clone(),
                scene_correspondence(p, scene, index, &format!("output/{}", scene.unit)).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut source = p.source();
    let planned =
        reel::tig_episode::require_current_scenes(&mut source, &request, &proofs).unwrap();
    assert_eq!(planned.components.len(), 4);
    exercise_episode_context(p, &request);
    assert_eq!(
        source.byte_requests(),
        2,
        "only native episode/conform controls are read"
    );
    assert!(
        !planned.production_currency_verified
            && !planned.executable
            && !planned.authorizes_eviction
    );
    let proof = proofs.remove("scene-b").unwrap();
    assert!(reel::tig_episode::require_current_scenes(&mut p.source(), &request, &proofs).is_err());
    proofs.insert("wrong-unit".into(), proof);
    assert!(reel::tig_episode::require_current_scenes(&mut p.source(), &request, &proofs).is_err());
    let proof = proofs.remove("wrong-unit").unwrap();
    proofs.insert("scene-b".into(), proof);
    let path = p.repo.path().join("records/output-scene-b.tig");
    let original = read(&path);
    let mut annotated = original.clone();
    annotated["status"] = json!({"production_state":"current"});
    write(p.repo.path(), "records/output-scene-b.tig", &annotated);
    p.commit("annotate selected output without changing its subject");
    assert!(check(p, "output/scene-b").is_ok());
    assert!(
        reel::tig_episode::require_current_scenes(&mut p.source(), &request, &proofs).is_err(),
        "refresh metadata correspondence after a source checkpoint changes"
    );
    annotated["selected"] = json!(Object::from_bytes(b"substituted master NOT PRESENT"));
    write(p.repo.path(), "records/output-scene-b.tig", &annotated);
    p.commit("substitute only selected output bytes");
    assert!(check(p, "output/scene-b").is_err());
    write(p.repo.path(), "records/output-scene-b.tig", &original);
    let mut cel = read(p.repo.path().join("records/cel-b.tig"));
    cel["selected"] = json!(Object::from_bytes(b"changed primary cel NOT PRESENT"));
    write(p.repo.path(), "records/cel-b.tig", &cel);
    p.commit("change only primary cel while retaining the previous scene master");
    assert!(
        check(p, "output/scene-b")
            .unwrap_err()
            .to_string()
            .contains("stale current-source recipe")
    );
    git(p.repo.path(), &["switch", "main"]);
    p.profile = original_profile;
    assert!(check(p, "output/scene-b").is_ok());
}

fn exercise_episode_context(p: &Project, request: &reel::tig_episode::Request) {
    let root = p.data.path().join("episode-context-metadata");
    let mut store = tig_store::metadata::MetadataStore::create(
        &root,
        tig_store::metadata::Limits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65_536,
        },
    )
    .unwrap();
    let mut source = p.source();
    let context = reel::tig_episode::freeze_composition_context(
        &mut source,
        request,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    let producer = Object::from_bytes(p.programs.manifest_bytes());
    let recipe =
        reel::tig_episode::composition_recipe(&mut p.source(), request, &context, &producer)
            .unwrap();
    assert!(
        context.requirements().primary().is_empty(),
        "assembly controls cannot substitute for child primary inputs"
    );
    let profile = serde_json::from_value(json!({
        "schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":context.archive(),
        "checkpoint":source.checkpoint(),"admitted":p.profile["admitted"],
        "byte_source":p.profile["byte_source"],
    }))
    .unwrap();
    let checkpoint = store.finish().unwrap();
    drop(store);
    drop(source);
    let mut reader = MetadataReader::open(&root, &checkpoint).unwrap();
    let (mut replayed_source, replayed) = reel::tig_episode::restore_composition_context(
        profile,
        request,
        context.selection(),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    let replayed_recipe =
        reel::tig_episode::composition_recipe(&mut replayed_source, request, &replayed, &producer)
            .unwrap();
    assert_eq!(recipe.fingerprint(), replayed_recipe.fingerprint());
    assert_eq!(context.selection(), replayed.selection());
}

fn add_episode_assembly(p: &mut Project) -> reel::tig_episode::Request {
    let schema =
        Object::from_bytes(&fs::read(p.repo.path().join("definitions/authored.json")).unwrap());
    let mut definitions = BTreeMap::new();
    for (name, fields) in [
        ("episode-source", vec!["episode_id", "scene_ids"]),
        (
            "assembly",
            vec!["episode", "language", "conform_slot", "components"],
        ),
    ] {
        let pin = write(
            p.repo.path(),
            &format!("definitions/{name}.json"),
            &json!({"schema":"tig.definition.design-1","id":format!("types/{name}"),"authored_schema":schema,"authoring":{"properties":fields,"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        p.profile["admitted"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":format!("types/{name}"),"object":pin,"payload":{"kind":"selected"}}));
        definitions.insert(name, pin);
    }
    let native = p.publish(
        "hot",
        &fs::read(p.data.path().join("controls/episode.json")).unwrap(),
    );
    let conform = p.publish(
        "hot",
        &fs::read(p.data.path().join("episode-v1.json")).unwrap(),
    );
    let record = |name: &str, id: &str, authored: Value, selected: Value, streams: Value| json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":format!("types/{name}"),"sha256":definitions[name].sha256,"bytes":definitions[name].bytes},"authored":authored,"selected":selected,"streams":streams,"status":{},"metadata":{}});
    write(
        p.repo.path(),
        "records/episode-source.tig",
        &record(
            "episode-source",
            "source/episode",
            json!({"episode_id":"episode-1","scene_ids":UNITS.iter().map(|id| json!({"id":format!("scene/{id}")})).collect::<Vec<_>>()}),
            json!(native),
            json!([{"slot":"retained-es","role":"evidence/conform","group":"source","object":conform}]),
        ),
    );
    write(
        p.repo.path(),
        "records/assembly.tig",
        &record(
            "assembly",
            "assemble/es",
            json!({"episode":{"id":"source/episode"},"language":"es","conform_slot":"retained-es","components":UNITS.iter().map(|id| ((*id).to_string(),json!({"id":format!("output/{id}")}))).collect::<BTreeMap<_,_>>()}),
            Value::Null,
            json!([]),
        ),
    );
    p.commit("bind declarative episode assembly to independently versioned component outputs");
    serde_json::from_value(json!({"schema":reel::tig_episode::SCHEMA,"assembly_id":"assemble/es","assembly_definition":"types/assembly","episode_definition":"types/episode-source","scene_definitions":["types/scene"],"component_definitions":["types/output"]})).unwrap()
}

fn publication_failures_and_parallel_writers(p: &Project) {
    use std::{
        io::{self, Read},
        sync::mpsc,
        time::Duration,
    };
    struct Interrupted {
        prefix: Cursor<Vec<u8>>,
    }
    impl Read for Interrupted {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.prefix.position() < self.prefix.get_ref().len() as u64 {
                self.prefix.read(out)
            } else {
                Err(io::Error::other("fixture interrupted producer"))
            }
        }
    }
    let bytes = b"fixture interrupted publication with a complete retry";
    let request = tig_store::Request {
        operation: key("interrupted-publication"),
        owner: key("writer-a"),
        object: Object::from_bytes(bytes),
        required: AckLevel::ProcessRestart,
    };
    let mut broken = Interrupted {
        prefix: Cursor::new(bytes[..8].to_vec()),
    };
    assert!(tig_store::publish(&mut p.publisher("hot"), &request, &mut broken).is_err());
    assert!(!object_file(&p.hot(), &request.object).exists());
    assert!(
        tig_store::recover(&mut p.publisher("hot"), &request)
            .unwrap()
            .is_none()
    );
    tig_store::publish(&mut p.publisher("hot"), &request, &mut Cursor::new(bytes)).unwrap();
    assert!(
        tig_store::recover(&mut p.publisher("hot"), &request)
            .unwrap()
            .is_some()
    );

    struct Paused {
        bytes: Cursor<Vec<u8>>,
        entered: Option<mpsc::Sender<()>>,
        release: mpsc::Receiver<()>,
    }
    impl Read for Paused {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).map_err(io::Error::other)?;
                self.release
                    .recv_timeout(Duration::from_secs(20))
                    .map_err(io::Error::other)?;
            }
            self.bytes.read(out)
        }
    }
    let (entered, waiting) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (completed, completion) = mpsc::channel();
    let mut first = p.publisher("hot");
    let mut second = p.publisher("hot");
    let slow = b"parallel paused scene writer";
    let fast = b"parallel independent cue writer";
    let make_request = |name: &str, bytes: &[u8]| tig_store::Request {
        operation: key(name),
        owner: key(name),
        object: Object::from_bytes(bytes),
        required: AckLevel::ProcessRestart,
    };
    let a = make_request("parallel-scene", slow);
    let b = make_request("parallel-cue", fast);
    std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            let mut reader = Paused {
                bytes: Cursor::new(slow.to_vec()),
                entered: Some(entered),
                release: released,
            };
            tig_store::publish(&mut first, &a, &mut reader)
        });
        let started = waiting.recv_timeout(Duration::from_secs(10));
        let two = scope.spawn(|| {
            let result = tig_store::publish(&mut second, &b, &mut Cursor::new(fast));
            completed.send(result.is_ok()).unwrap();
            result
        });
        let independent = completion.recv_timeout(Duration::from_secs(10));
        // Always unblock and join, including failure paths: no abandoned worker.
        let _ = release.send(());
        let first_result = one.join().unwrap();
        let second_result = two.join().unwrap();
        assert!(started.is_ok());
        assert!(
            independent.unwrap(),
            "unrelated writes must finish while writer A is paused"
        );
        first_result.unwrap();
        second_result.unwrap();
    });
    for request in [&a, &b] {
        assert!(
            tig_store::recover(&mut p.publisher("hot"), request)
                .unwrap()
                .is_some()
        );
    }
}

fn decoded(path: &Path, audio: bool) -> Vec<u8> {
    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-nostdin", "-i"]).arg(path);
    if audio {
        command.args(["-map", "0:a:0", "-f", "s24le", "-"]);
    } else {
        command.args([
            "-map",
            "0:v:0",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ]);
    }
    let output = command.output().unwrap();
    assert!(output.status.success());
    output.stdout
}

#[test]
#[ignore = "requires native FFmpeg; complete fake episode version/render/Git/cache lifecycle"]
fn fake_episode_runs_v1_then_rebuilds_only_the_changed_scene() {
    let mut p = Project::new();
    publication_failures_and_parallel_writers(&p);
    let mut scenes: Vec<_> = (0..4).map(|i| p.render(i, "v1")).collect();
    let (master_v1, path_v1) = p.assemble(&scenes, "v1");
    let manifest_v1 = p.deliver(&master_v1, &path_v1);
    p.commit("review delivery v1 source and produced scene records");
    exercise_scene_correspondence(&mut p, &scenes);
    let source_v1 = p.profile.clone();
    let remote = p.data.path().join("remote.git");
    git(p.data.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(
        p.repo.path(),
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(p.repo.path(), &["push", "-u", "origin", "main"]);
    git(p.repo.path(), &["switch", "-c", "fix-cel-b"]);
    let replacement = p.publish("hot", &picture([0, 255, 0]));
    let mut leaf = read(p.repo.path().join("records/cel-b.tig"));
    leaf["selected"] = json!(replacement);
    write(p.repo.path(), "records/cel-b.tig", &leaf);
    p.commit("replace one cel");
    let producer = Object::from_bytes(p.programs.manifest_bytes());
    let mut dirty = Vec::new();
    for (i, s) in scenes.iter().enumerate() {
        let current = tig_scene::recipe(&mut p.source(), &p.requests[i], &producer).unwrap();
        let original = s.claim.check_plan(&s.recipe).unwrap();
        let state = tig_assets::graph::recipe::produced::compare(
            &current,
            Some(&s.subject),
            Some(&original),
        )
        .unwrap();
        if state == Correspondence::StaleRecipe {
            dirty.push(i);
        } else {
            assert_eq!(state, Correspondence::ClaimedRecipeMatch);
        }
    }
    assert_eq!(dirty, vec![2]);
    let stale: Job = serde_json::from_slice(&fs::read(&scenes[2].job).unwrap()).unwrap();
    let mut reject = p.source();
    assert!(reject.prepare(&stale).is_err());
    assert_eq!(reject.byte_requests(), 0);
    drop(reject);
    let old_primary: BTreeSet<_> = scenes
        .iter()
        .flat_map(|s| s.primary.iter().cloned())
        .collect();
    let old_inputs: BTreeSet<_> = scenes
        .iter()
        .flat_map(|s| s.primary.union(&s.controls).cloned())
        .collect();
    let changed = p.render(2, "v2");
    scenes[2] = changed;
    let (master_v2, path_v2) = p.assemble(&scenes, "v2");
    assert_ne!(master_v1, master_v2);
    assert_eq!(decoded(&path_v1, true), decoded(&path_v2, true));
    let before = decoded(&path_v1, false);
    let after = decoded(&path_v2, false);
    let scene_bytes = 24 * 64 * 64 * 3;
    assert_eq!(before.len(), scene_bytes * 4);
    assert_eq!(before.len(), after.len());
    for i in 0..4 {
        assert_eq!(
            before[i * scene_bytes..(i + 1) * scene_bytes]
                == after[i * scene_bytes..(i + 1) * scene_bytes],
            i != 2
        );
    }
    let manifest_v2 = p.deliver(&master_v2, &path_v2);
    p.commit("qualify rebuilt scene and episode v2");
    git(p.repo.path(), &["push", "-u", "origin", "fix-cel-b"]);
    git(p.repo.path(), &["switch", "main"]);
    git(
        p.repo.path(),
        &[
            "merge",
            "--no-ff",
            "fix-cel-b",
            "-m",
            "merge qualified fake episode fix",
        ],
    );
    git(p.repo.path(), &["push", "origin", "main"]);
    let new_primary: BTreeSet<_> = scenes
        .iter()
        .flat_map(|s| s.primary.iter().cloned())
        .collect();
    assert_eq!(old_primary.difference(&new_primary).count(), 1);
    assert_eq!(
        new_primary
            .difference(&old_primary)
            .cloned()
            .collect::<Vec<_>>(),
        vec![replacement]
    );
    let new_inputs: BTreeSet<_> = scenes
        .iter()
        .flat_map(|s| s.primary.union(&s.controls).cloned())
        .collect();
    storage_transition(
        &p,
        &old_inputs,
        &new_inputs,
        &master_v1,
        &master_v2,
        [&manifest_v1, &manifest_v2],
    );
    let mut initial = Source::open(serde_json::from_value(source_v1).unwrap()).unwrap();
    let expected: reel::scene_delivery::FileRef =
        serde_json::from_value(reference("episode/fake", &master_v1)).unwrap();
    let held = initial
        .prepare_files(std::slice::from_ref(&expected))
        .unwrap();
    assert_eq!(
        Object::from_bytes(&fs::read(held.verified_path(&expected).unwrap()).unwrap()),
        master_v1
    );
    let tracked = git(p.repo.path(), &["ls-files"]);
    assert!(
        tracked
            .lines()
            .all(|s| s.starts_with("records/") || s.starts_with("definitions/"))
    );
    assert!(
        tracked
            .lines()
            .all(|s| s.ends_with(".tig") || s.ends_with(".json"))
    );
}
