//! A complete tiny project. All stores, Git remotes and media are temporary.
use super::*;
use reel::{
    episode_conform, media_runtime, tig_publication, tig_scene,
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
    owners: SourceJournal,
}
struct Scene {
    unit: String,
    job: PathBuf,
    output: PathBuf,
    recipe: Plan,
    claim: Claim,
    subject: BoundSubject,
    primary: BTreeSet<Object>,
    controls: BTreeSet<Object>,
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
        let programs = media_runtime::BoundPrograms::bind(&media_runtime::Bindings {
            host: media_runtime::ByteIdentity::observe_file(&std::env::current_exe().unwrap())
                .unwrap(),
            ffmpeg: file(locate("ffmpeg")),
            ffprobe: file(locate("ffprobe")),
            dependencies: Default::default(),
        })
        .unwrap();
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
        let base = read(project.repo.path().join("records/scene.tig"));
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
            &json!({"schema":"tig.definition.design-1","id":"types/output","authored_schema":schema,"authoring":{"properties":["scene"],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
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
                &json!({"schema":"tig.instance.design-1","id":format!("output/{unit}"),"definition":{"id":"types/output","sha256":output_def.sha256,"bytes":output_def.bytes},"authored":{"scene":{"id":format!("scene/{unit}")}},"selected":null,"streams":[],"status":{},"metadata":{}}),
            );
        }
        fs::remove_file(project.repo.path().join("records/scene.tig")).unwrap();
        project.commit("declare tiny episode and independently versioned inputs");
        project
    }
    fn render(&mut self, index: usize, version: &str) -> Scene {
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
        let published = job
            .publish(
                &mut self.owners,
                &candidate,
                &[OutputBinding {
                    output_name: "master.mkv",
                    subject: &subject,
                }],
                &Publication {
                    operation: key(&format!("render-{unit}-{version}")),
                    owner: key("fixture"),
                    required: AckLevel::ProcessRestart,
                },
                &mut backend,
            )
            .unwrap();
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
            primary,
            controls,
        }
    }
    fn assemble(&self, scenes: &[Scene], version: &str) -> (Object, PathBuf) {
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
        let segments:Vec<_>=scenes.iter().map(|s|json!({"kind":"scene","id":s.unit,"master":pin(root,&s.output.join("master.mkv")),"source_receipt":pin(root,&s.output.join("receipt.json")),"delivery_job":pin(root,&s.job),"delivery_receipt":pin(root,&s.output.join("receipt.json")),"semantic_delivery":pin(root,&s.job.parent().unwrap().join("semantic-delivery.json"))})).collect();
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
        let receipt = episode_conform::build_with_scene_inputs(
            &root.join(name),
            root,
            root,
            &output,
            &provided,
        )
        .unwrap();
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
    let root = p.state();
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
    // A completed publication marker is not enough when the retained bytes vanish.
    let previous = object_file(&p.data.path().join("history"), old);
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
    for hash in &expected {
        assert!(reel_assets::object(&p.hot(), hash).exists());
    }
    fs::rename(missing, &previous).unwrap();
    demotion::run(
        &root,
        &root.join("request.json"),
        &root.join(".local/apply"),
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
