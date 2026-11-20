//! Synthetic complete issue: two retained part results, one admitted assembler.
use super::*;
use reel_publication::{
    ComicPage, Content, Edition, PageKind, Publication, ReadingDirection,
    comic_issue_source::{Assembly, Part, Plan as IssuePlan},
    comic_source::{Layer, LayerRole, Limits, PageSource, Panel, Pin, Request, Utterance},
};
use reel_workflow_native::{NativeProfile, admission, graph, retention, staging, upstream, worker};
use std::collections::BTreeSet;

#[derive(Default)]
struct Assets {
    values: BTreeMap<String, (Artifact, Vec<u8>)>,
    paths: BTreeMap<String, String>,
}
impl Assets {
    fn put(&mut self, id: &str, path: &str, media: &str, raw: Vec<u8>) -> Artifact {
        let asset = Artifact {
            logical_id: id.into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: media.into(),
        };
        assert!(
            self.values
                .insert(id.into(), (asset.clone(), raw))
                .is_none()
        );
        assert!(self.paths.insert(path.into(), id.into()).is_none());
        asset
    }
    fn pin(&self, path: &str) -> Pin {
        let asset = &self.values[&self.paths[path]].0;
        Pin {
            path: path.into(),
            sha256: asset.sha256.clone(),
            bytes: asset.bytes,
        }
    }
    fn read(&self, path: &str) -> Vec<u8> {
        self.values[&self.paths[path]].1.clone()
    }
    fn snapshot(&self, fixture: &Fixture, inputs: &[Artifact]) -> SnapshotProfile {
        let mut profile = fixture.profile.clone();
        let ids: BTreeSet<_> = inputs.iter().map(|a| &a.logical_id).collect();
        profile.records.clear();
        profile.bindings.clear();
        profile.input_streams.clear();
        for (i, (id, _)) in self.values.iter().enumerate() {
            if ids.contains(id) {
                profile.records.push(format!("records/issue-{i}.tig"));
                profile.bindings.insert(
                    id.clone(),
                    Binding {
                        selected: LogicalPath::parse(&format!("payloads/issue-{i}.bin")).unwrap(),
                        streams: BTreeMap::new(),
                    },
                );
                profile.input_streams.insert(id.clone(), None);
            }
        }
        profile
    }
}
fn run(exe: &Path, args: &[&std::ffi::OsStr]) -> Vec<u8> {
    let output = Command::new(exe).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "status {}; stdout {}; stderr {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn save(path: &Path, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn clone_source(source: &NativeProfile) -> NativeProfile {
    serde_json::from_slice(&serde_json::to_vec(source).unwrap()).unwrap()
}
fn publish_input(root: &Path, source: &NativeProfile, raw: &[u8]) {
    let directory = root.join("source-ingress");
    fs::create_dir_all(&directory).unwrap();
    let name = format!("{}.bin", digest(raw));
    fs::write(directory.join(&name), raw).unwrap();
    tig_git::annex::workflow::publication::publish(
        &tig_git::annex::workflow::publication::Profile {
            schema: "tig.annex-file-publication.experimental-1".into(),
            pool: source.source.pool.clone(),
            store: source.source.store.clone(),
            source_root: directory,
            source: LogicalPath::parse(&name).unwrap(),
            source_owned: true,
            expected: Some(Object::from_bytes(raw)),
            include_private_location: false,
            maximum_bytes: 32 * 1024 * 1024,
            minimum_free_bytes: 100 * 1024 * 1024,
        },
    )
    .unwrap();
}
struct Parent {
    packet: Packet,
    source: NativeProfile,
    profile: admission::Profile,
    receipt: admission::Receipt,
    result: ReturnedResult,
    ack: retention::Acknowledgement,
}

#[test]
#[ignore = "Complete synthetic two-part comic issue in explicitly declared existing native pool; no project adoption or automatic actor allocation"]
fn complete_issue_consumes_retained_parts_and_restores_editable_packets_from_bare_history() {
    complete_issue(false, false);
}

#[test]
#[ignore = "Synthetic retained derived-control job and final issue assembly in the explicitly declared existing native pool"]
fn derived_issue_control_is_retained_upstream_before_final_issue_assembly() {
    complete_issue(true, false);
}

#[test]
#[ignore = "Fresh synthetic v2 issue/control lifecycle in the explicitly declared existing native pool; preserves original operation roots and frozen runtime, no project adoption"]
fn intent_bound_issue_control_retains_and_replays_original_producers_through_native_history() {
    complete_issue(true, true);
}

fn complete_issue(derived_control: bool, intent_bound: bool) {
    assert!(!intent_bound || derived_control);
    let descriptor: serde_json::Value = serde_json::from_slice(
        &fs::read(std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("native fixture required"))
            .unwrap(),
    )
    .unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(descriptor["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(descriptor["annex"].clone()).unwrap();
    let built_executable = PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native"));
    let executable = std::env::var_os("REEL_NATIVE_ISSUE_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| built_executable.clone());
    let worker_raw = fs::read(&executable).unwrap();
    assert_eq!(
        digest(&worker_raw),
        digest(&fs::read(&built_executable).unwrap())
    );
    let tool = Artifact {
        logical_id: "tool/native-issue".into(),
        sha256: digest(&worker_raw),
        bytes: worker_raw.len() as u64,
        media_type: "application/octet-stream".into(),
    };
    let mut fixture = std::mem::ManuallyDrop::new(Fixture::new());
    // Preserve all original handles even on panic: uncertain effects must never
    // lose the original operation/index/control namespace to TempDir::drop.
    let fixture_root = fixture.temp.path().to_path_buf();
    let mut assets = Assets::default();
    let font = std::env::var_os("REEL_COMIC_TEST_FONT")
        .map(PathBuf::from)
        .unwrap_or_else(|| "C:/Windows/Fonts/arial.ttf".into());
    assets.put(
        "comic/font",
        "fonts/body.ttf",
        "font/ttf",
        fs::read(font).unwrap(),
    );
    assets.put(
        "comic/license",
        "fonts/license.txt",
        "text/plain",
        b"Private synthetic installed-font test; no redistribution rights granted.".to_vec(),
    );
    let mut pixels = resvg::tiny_skia::Pixmap::new(72, 72).unwrap();
    pixels.fill(resvg::tiny_skia::Color::from_rgba8(230, 240, 244, 255));
    assets.put(
        "comic/art",
        "art/background.png",
        "image/png",
        pixels.encode_png().unwrap(),
    );
    let mut pages = Vec::new();
    for (id, line) in [("page01", "La luz sigue."), ("page02", "Seguimos.")] {
        let page = PageSource {
            schema: "reel.comic-page.v1".into(),
            id: id.into(),
            width: 200,
            height: 200,
            panels: vec![Panel {
                id: "panel-a".into(),
                rect: [8, 8, 184, 184],
            }],
            layers: vec![Layer {
                id: "background".into(),
                panel_id: "panel-a".into(),
                role: LayerRole::Background,
                source: assets.pin("art/background.png"),
                rect: [8, 8, 184, 184],
            }],
            utterances: vec![Utterance {
                id: "line-a".into(),
                panel_id: "panel-a".into(),
                speaker: "Narrador".into(),
                lines: vec![line.into()],
                rect: [20, 20, 160, 50],
                font_size: 14,
                tail: None,
            }],
        };
        let asset = assets.put(
            &format!("comic/{id}"),
            &format!("pages/{id}.json"),
            "application/json",
            serde_json::to_vec_pretty(&page).unwrap(),
        );
        pages.push(ComicPage {
            id: id.into(),
            kind: PageKind::Story,
            source: asset,
            panel_order: vec!["panel-a".into()],
            utterance_order: vec!["line-a".into()],
            spread_id: None,
        });
    }
    let final_contracts: OutputContracts = [
        ("reading.pdf", "application/pdf"),
        ("transcript.json", "application/json"),
    ]
    .into_iter()
    .map(|(name, media)| {
        (
            name.into(),
            OutputContract {
                media_type: media.into(),
                max_bytes: 16 * 1024 * 1024,
                required: true,
            },
        )
    })
    .collect();
    let publication = Publication {
        schema: reel_publication::PUBLICATION_SCHEMA.into(),
        id: "synthetic-issue".into(),
        editions: vec![Edition {
            id: "es".into(),
            language: "es".into(),
            content: Content::Comic {
                reading_direction: ReadingDirection::LeftToRight,
                pages,
            },
            outputs: final_contracts.clone(),
        }],
    };
    let master = assets.put(
        "comic/master",
        "publication.json",
        "application/json",
        serde_json::to_vec_pretty(&publication).unwrap(),
    );
    let plan = IssuePlan {
        schema: "reel.comic-issue-plan.v1".into(),
        publication,
        edition_id: "es".into(),
        parts: vec![
            Part {
                id: "opening".into(),
                pages: vec!["page01".into()],
            },
            Part {
                id: "closing".into(),
                pages: vec!["page02".into()],
            },
        ],
    };
    let plan_asset = assets.put(
        "comic/plan",
        "issue-plan.json",
        "application/json",
        serde_json::to_vec_pretty(&plan).unwrap(),
    );
    let mut part_data = Vec::new();
    let mut issue_paths =
        BTreeMap::from([("issue-plan.json".into(), plan_asset.logical_id.clone())]);
    let mut receipt_pins = BTreeMap::new();
    let mut bundle_pins = BTreeMap::new();
    let mut upstream_refs = BTreeMap::new();
    for part in &plan.parts {
        let request = Request {
            editable_packet: true,
            schema: "reel.comic-build.v1".into(),
            part_id: Some(part.id.clone()),
            publication_id: plan.publication.id.clone(),
            edition_id: "es".into(),
            language: "es".into(),
            font_family: std::env::var("REEL_COMIC_TEST_FONT_FAMILY")
                .unwrap_or_else(|_| "Arial".into()),
            font: assets.pin("fonts/body.ttf"),
            font_license: assets.pin("fonts/license.txt"),
            pages: part
                .pages
                .iter()
                .map(|p| assets.pin(&format!("pages/{p}.json")))
                .collect(),
            limits: Limits {
                input_bytes: 8 * 1024 * 1024,
                output_bytes: 16 * 1024 * 1024,
            },
        };
        let request_raw = serde_json::to_vec_pretty(&request).unwrap();
        let request_asset = assets.put(
            &format!("comic/{}/request", part.id),
            &format!("requests/{}.json", part.id),
            "application/json",
            request_raw.clone(),
        );
        let (receipt, files) =
            reel_publication::comic::capture_with(&request_raw, |pin| Ok(assets.read(&pin.path)))
                .unwrap();
        let mut source_paths = BTreeMap::new();
        let mut source_inputs = BTreeMap::from([
            (master.logical_id.clone(), master.clone()),
            (plan_asset.logical_id.clone(), plan_asset.clone()),
            (request_asset.logical_id.clone(), request_asset.clone()),
        ]);
        for path in receipt.source_files.keys() {
            let asset = if path == "request.json" {
                request_asset.clone()
            } else {
                assets.values[&assets.paths[path]].0.clone()
            };
            if path != "request.json" {
                source_paths.insert(path.clone(), asset.logical_id.clone());
            }
            source_inputs.insert(asset.logical_id.clone(), asset);
        }
        let receipt_asset = assets.put(
            &format!("comic/{}/receipt", part.id),
            &format!("parts/{}/receipt.json", part.id),
            "application/json",
            files["receipt.json"].clone(),
        );
        receipt_pins.insert(
            part.id.clone(),
            Pin {
                path: format!("parts/{}/receipt.json", part.id),
                sha256: receipt_asset.sha256.clone(),
                bytes: receipt_asset.bytes,
            },
        );
        issue_paths.insert(
            format!("parts/{}/receipt.json", part.id),
            receipt_asset.logical_id.clone(),
        );
        upstream_refs.insert(
            receipt_asset.logical_id.clone(),
            reel_workflow::upstream::Reference {
                task_id: part.id.clone(),
                artifact: reel_workflow::upstream::Selector::WorkerReceipt,
            },
        );
        for (name, a) in &receipt.outputs {
            assets.put(
                &a.logical_id,
                &format!("parts/{}/{name}", part.id),
                &a.media_type,
                files[name].clone(),
            );
            if name == "editable.packet" {
                issue_paths.insert(format!("parts/{}/{name}", part.id), a.logical_id.clone());
                bundle_pins.insert(
                    part.id.clone(),
                    Pin {
                        path: format!("parts/{}/{name}", part.id),
                        sha256: a.sha256.clone(),
                        bytes: a.bytes,
                    },
                );
                upstream_refs.insert(
                    a.logical_id.clone(),
                    reel_workflow::upstream::Reference {
                        task_id: part.id.clone(),
                        artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
                    },
                );
            }
        }
        let outputs = receipt
            .outputs
            .iter()
            .map(|(name, a)| {
                (
                    name.clone(),
                    OutputContract {
                        media_type: a.media_type.clone(),
                        max_bytes: 16 * 1024 * 1024,
                        required: true,
                    },
                )
            })
            .collect();
        let task = Task {
            id: part.id.clone(),
            native_inputs: None,
            upstream_inputs: None,
            operation: Operation::DeterministicTool,
            tool: Some(ToolContract {
                auxiliary_executables: BTreeMap::new(),
                schema: "reel.workflow-tool.v1".into(),
                capability: ToolCapability::ComicRender,
                executable: tool.clone(),
                publication: master.clone(),
                edition_id: "es".into(),
                request: request_asset,
                source_bindings: source_paths,
                part: Some(ComicPart {
                    schema: "reel.workflow-comic-part.v1".into(),
                    plan: plan_asset.clone(),
                    part_id: part.id.clone(),
                }),
            }),
            language: Some("es".into()),
            prompt: String::new(),
            inputs: source_inputs.into_values().collect(),
            depends_on: vec![],
            outputs,
        };
        part_data.push((task, receipt, receipt_asset, files));
    }
    assert_ne!(
        part_data[0].1.outputs["reading.pdf"].logical_id,
        part_data[1].1.outputs["reading.pdf"].logical_id
    );
    let intent_asset = intent_bound.then(|| {
        let intent = reel_publication::composition::Intent {
            schema: "reel.workflow-composition-intent.v1".into(),
            project: "synthetic-comic".into(),
            format: reel_publication::composition::Format::ComicIssue,
            editions: BTreeMap::from([(
                "es".into(),
                reel_publication::composition::Edition {
                    language: "es".into(),
                    components: plan.parts.iter().map(|p| p.id.clone()).collect(),
                    controls: vec!["issue-control".into()],
                    final_task: "issue-assemble".into(),
                },
            )]),
        };
        intent.check().unwrap();
        let asset = assets.put(
            "comic/composition-intent",
            "intent.json",
            "application/json",
            serde_json::to_vec(&intent).unwrap(),
        );
        issue_paths.insert("intent.json".into(), asset.logical_id.clone());
        asset
    });
    let composition =
        intent_asset
            .as_ref()
            .map(|_| reel_publication::comic_issue_source::Composition {
                intent: assets.pin("intent.json"),
                producers: plan
                    .parts
                    .iter()
                    .map(|p| (p.id.clone(), p.id.clone()))
                    .collect(),
            });
    let assembly = Assembly {
        composition: composition.clone(),
        bundles: bundle_pins,
        schema: if intent_bound {
            "reel.comic-issue-assembly.v2"
        } else {
            "reel.comic-issue-assembly.v1"
        }
        .into(),
        plan: assets.pin("issue-plan.json"),
        parts: receipt_pins,
        limits: Limits {
            input_bytes: 32 * 1024 * 1024,
            output_bytes: 32 * 1024 * 1024,
        },
    };
    let assembly_raw = serde_json::to_vec_pretty(&assembly).unwrap();
    let mut assembly_asset = assets.put(
        "comic/assembly",
        "assembly-request.json",
        "application/json",
        if derived_control {
            serde_json::to_vec(&reel_workflow::issue_control::Request {
                composition,
                schema: if intent_bound {
                    "reel.workflow-issue-control.v2"
                } else {
                    "reel.workflow-issue-control.v1"
                }
                .into(),
                plan_path: "issue-plan.json".into(),
                parts: plan
                    .parts
                    .iter()
                    .map(|part| {
                        (
                            part.id.clone(),
                            reel_workflow::issue_control::PartPaths {
                                receipt: format!("parts/{}/receipt.json", part.id),
                                bundle: format!("parts/{}/editable.packet", part.id),
                            },
                        )
                    })
                    .collect(),
                limits: Limits {
                    input_bytes: 32 * 1024 * 1024,
                    output_bytes: 32 * 1024 * 1024,
                },
            })
            .unwrap()
        } else {
            assembly_raw.clone()
        },
    );
    let definition =
        Object::from_bytes(&fs::read(fixture_root.join("definitions/input.json")).unwrap());
    for (i, (id, (asset, raw))) in assets.values.iter().enumerate() {
        write(
            &fixture_root,
            &format!("records/issue-{i}.tig"),
            json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":"types/input","sha256":definition.sha256,"bytes":definition.bytes},"authored":{"text":"synthetic complete issue"},"selected":Object::from_bytes(raw),"streams":[],"status":{},"metadata":{}}),
        );
        fs::write(
            fixture_root.join(format!("payloads/issue-{i}.bin")),
            Key::from_object(&Object::from_bytes(raw)).unlocked_pointer(),
        )
        .unwrap();
        assert_eq!(asset.sha256, digest(raw));
    }
    git(&fixture_root, &["add", "records", "payloads"]);
    git(&fixture_root, &["commit", "-m", "synthetic complete issue"]);
    fixture.profile.commit = git(&fixture_root, &["rev-parse", "HEAD"]);
    let raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    for (task, _, _, _) in &mut part_data {
        task.native_inputs = Some(Fixture::declaration(
            &assets.snapshot(&fixture, &task.inputs),
        ));
    }
    let case = std::mem::ManuallyDrop::new(AdmissionCase::with_extra_slot(
        &fixture,
        native.store.clone(),
        None,
        true,
    ));
    let case_root = case.temp.path().to_path_buf();
    eprintln!(
        "Native issue fixture: {}; sources: {}",
        case_root.display(),
        fixture_root.display()
    );
    if let Some(evidence) = std::env::var_os("REEL_NATIVE_ISSUE_EVIDENCE_DIRECTORY") {
        let evidence = PathBuf::from(evidence);
        assert!(evidence.is_absolute() && evidence.is_dir());
        save(
            &evidence.join("original-operation-roots.json"),
            &json!({"root":case_root,"fixture":fixture_root,"runtime_sha256":tool.sha256}),
        );
    }
    // These two roots are evidence, not disposable test scratch, including failure.
    fs::write(
        case_root.join("fixture-root.txt"),
        fixture_root.to_string_lossy().as_bytes(),
    )
    .unwrap();
    let control = case.source.source.pool.control.clone();
    let mut pool = native;
    pool.pool.control = control;
    pool.maximum_bytes = 32 * 1024 * 1024;
    let mut parents = Vec::new();
    let mut graph_bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::new(),
    };
    let mut parent_plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: "synthetic-comic".into(),
        source_commit: fixture.profile.commit.clone(),
        tasks: part_data.iter().map(|p| p.0.clone()).collect(),
    };
    let mut published = BTreeSet::new();
    for (index, (task, receipt, worker_receipt, files)) in part_data.iter().enumerate() {
        let packet = make_packet(&parent_plan, &task.id, BTreeMap::new()).unwrap();
        let source = NativeProfile {
            movie_verifier: None,
            snapshot: assets.snapshot(&fixture, &task.inputs),
            annex: annex.clone(),
            source: pool.clone(),
        };
        let mut profile = case.profile.clone();
        profile.scope.actor =
            tig_store::Key::parse(if index == 0 { "slot2" } else { "slot3" }).unwrap();
        profile.scope.operation = tig_store::Key::parse(&format!("part-{}", task.id)).unwrap();
        let admitted = admission::Prepared::open(&packet, &source, &profile, "attempt-1")
            .unwrap()
            .accept()
            .unwrap();
        for a in &task.inputs {
            if published.insert(a.sha256.clone()) {
                publish_input(&case_root, &source, &assets.values[&a.logical_id].1);
            }
        }
        let output = case_root.join(format!("part-{}", task.id));
        fs::create_dir(&output).unwrap();
        for (path, raw) in files {
            let target = output.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, raw).unwrap();
        }
        let result = ReturnedResult {
            schema: RESULT_SCHEMA.into(),
            task_id: task.id.clone(),
            task_key: packet.task_key.clone(),
            attempt_id: "attempt-1".into(),
            artifacts: receipt.outputs.clone(),
            provenance: Provenance {
                mode: Mode::Tool,
                provider: None,
                model: None,
                executable: Some(tool.clone()),
            },
            worker_receipt: Some(worker_receipt.clone()),
        };
        let returns = reel_workflow_native::response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: output,
            files: result
                .artifacts
                .keys()
                .map(|n| (n.clone(), n.clone()))
                .collect(),
        };
        let acks = case_root.join(format!("acks-{}", task.id));
        fs::create_dir(&acks).unwrap();
        let handoff = reel_workflow_native::handoff::Profile {
            schema: "reel.workflow-native-handoff.v1".into(),
            source: clone_source(&source),
            admission: profile.clone(),
            binding: reel_workflow_native::handoff::Binding {
                schema: "reel.workflow-native-handoff-binding.v1".into(),
                admission: admitted.clone(),
                target: retained_target(),
            },
            runtime: Tool {
                path: executable.clone(),
                sha256: tool.sha256.clone(),
                bytes: tool.bytes,
            },
            acknowledgements: acks.clone(),
        };
        let packet_path = case_root.join(format!("{}-packet.json", task.id));
        let profile_path = case_root.join(format!("{}-profile.json", task.id));
        let result_path = case_root.join(format!("{}-result.json", task.id));
        let returns_path = case_root.join(format!("{}-returns.json", task.id));
        let journal = case_root.join(format!("{}-journal", task.id));
        save(&packet_path, &packet);
        save(&profile_path, &handoff);
        save(&result_path, &result);
        save(&returns_path, &returns);
        run(
            &executable,
            &[
                "create-handoff".as_ref(),
                packet_path.as_os_str(),
                profile_path.as_os_str(),
                journal.as_os_str(),
                "--apply".as_ref(),
            ],
        );
        run(
            &executable,
            &[
                "import-handoff".as_ref(),
                journal.as_os_str(),
                profile_path.as_os_str(),
                result_path.as_os_str(),
                returns_path.as_os_str(),
                "--apply".as_ref(),
            ],
        );
        let ack: retention::Acknowledgement = serde_json::from_slice(
            &fs::read(acks.join(format!("retained-{}-attempt-1.json", packet.task_key))).unwrap(),
        )
        .unwrap();
        graph_bindings.tasks.insert(
            task.id.clone(),
            graph::Binding {
                journal,
                profile: graph::PinnedFile {
                    path: profile_path.clone(),
                    sha256: digest(&fs::read(profile_path).unwrap()),
                },
                staging_ticket: None,
            },
        );
        parents.push(Parent {
            packet,
            source,
            profile,
            receipt: admitted,
            result,
            ack,
        });
    }
    if derived_control {
        let bindings = reel_workflow::upstream::Bindings {
            streaming: None,
            schema: "reel.workflow-upstream-inputs.v1".into(),
            inputs: upstream_refs.clone(),
        };
        let refs = parents
            .iter()
            .map(|p| {
                (
                    p.packet.task.id.clone(),
                    upstream::Parent {
                        packet: &p.packet,
                        result: &p.result,
                        source: &p.source,
                        admission: &p.profile,
                        receipt: &p.receipt,
                        acknowledgement: &p.ack,
                    },
                )
            })
            .collect();
        let mut originals = vec![master.clone(), plan_asset.clone(), assembly_asset.clone()];
        if let Some(asset) = &intent_asset {
            originals.push(asset.clone());
        }
        let mut snapshot = assets.snapshot(&fixture, &originals);
        snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
        snapshot.upstream = Some(upstream::Frozen::capture(&bindings, &refs));
        let mut inputs: BTreeMap<_, _> = originals
            .into_iter()
            .map(|a| (a.logical_id.clone(), a))
            .collect();
        for id in issue_paths.values() {
            inputs.insert(id.clone(), assets.values[id].0.clone());
        }
        let binding_raw = bindings.canonical_bytes().unwrap();
        let task = Task {
            id: "issue-control".into(),
            native_inputs: Some(Fixture::declaration(&snapshot)),
            upstream_inputs: Some(Artifact {
                logical_id: "control/upstream".into(),
                sha256: digest(&binding_raw),
                bytes: binding_raw.len() as u64,
                media_type: "application/json".into(),
            }),
            operation: Operation::DeterministicTool,
            tool: Some(ToolContract {
                auxiliary_executables: BTreeMap::new(),
                schema: "reel.workflow-tool.v1".into(),
                capability: ToolCapability::ComicIssueControl,
                executable: tool.clone(),
                publication: master.clone(),
                edition_id: "es".into(),
                request: assembly_asset.clone(),
                source_bindings: issue_paths.clone(),
                part: None,
            }),
            language: Some("es".into()),
            prompt: String::new(),
            inputs: inputs.into_values().collect(),
            depends_on: parents.iter().map(|p| p.packet.task.id.clone()).collect(),
            outputs: BTreeMap::from([(
                "assembly.json".into(),
                OutputContract {
                    media_type: "application/json".into(),
                    required: true,
                    max_bytes: reel_workflow::MAX_CONTROL_BYTES as u64,
                },
            )]),
        };
        parent_plan.tasks.push(task);
        let dependencies = parents
            .iter()
            .map(|p| {
                (
                    p.packet.task.id.clone(),
                    DependencyResult {
                        result_sha256: canonical_digest(&p.result).unwrap(),
                        artifacts: p.result.artifacts.clone(),
                    },
                )
            })
            .collect();
        let packet = make_packet(&parent_plan, "issue-control", dependencies).unwrap();
        let source = NativeProfile {
            movie_verifier: None,
            snapshot,
            annex: annex.clone(),
            source: pool.clone(),
        };
        let mut profile = case.profile.clone();
        profile.scope.actor = tig_store::Key::parse("slot4").unwrap();
        profile.scope.operation = tig_store::Key::parse("derived-issue-control").unwrap();
        let prepared =
            upstream::Admission::prepare(&packet, &source, &profile, "attempt-1", refs).unwrap();
        let admitted = prepared.accept().unwrap();
        publish_input(
            &case_root,
            &source,
            &assets.values[&assembly_asset.logical_id].1,
        );
        if let Some(asset) = intent_asset
            .as_ref()
            .filter(|a| published.insert(a.sha256.clone()))
        {
            publish_input(&case_root, &source, &assets.values[&asset.logical_id].1);
        }
        let acks = case_root.join("control-acks");
        fs::create_dir(&acks).unwrap();
        let handoff = reel_workflow_native::handoff::Profile {
            schema: "reel.workflow-native-handoff.v1".into(),
            source: clone_source(&source),
            admission: profile.clone(),
            binding: reel_workflow_native::handoff::Binding {
                schema: "reel.workflow-native-handoff-binding.v1".into(),
                admission: admitted.clone(),
                target: retained_target(),
            },
            runtime: Tool {
                path: executable.clone(),
                sha256: tool.sha256.clone(),
                bytes: tool.bytes,
            },
            acknowledgements: acks.clone(),
        };
        let packet_path = case_root.join("control-packet.json");
        let profile_path = case_root.join("control-profile.json");
        let journal = case_root.join("control-journal");
        save(&packet_path, &packet);
        save(&profile_path, &handoff);
        run(
            &executable,
            &[
                "create-handoff".as_ref(),
                packet_path.as_os_str(),
                profile_path.as_os_str(),
                journal.as_os_str(),
                "--apply".as_ref(),
            ],
        );
        let scratch = case_root.join("control-input-scratch");
        fs::create_dir(&scratch).unwrap();
        let stage = staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent: scratch,
            maximum_bytes: 128 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 65536,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        };
        let stage_ticket = staging::prepare(&packet, &source, &profile, &admitted, &stage).unwrap();
        let stage_raw = stage_ticket.canonical_bytes().unwrap();
        staging::export(&packet, &source, &profile, &stage_raw, &digest(&stage_raw)).unwrap();
        let stage_path = case_root.join("control-input-ticket.json");
        save(&stage_path, &stage_ticket);
        graph_bindings.tasks.insert(
            packet.task.id.clone(),
            graph::Binding {
                journal,
                profile: graph::PinnedFile {
                    path: profile_path.clone(),
                    sha256: digest(&fs::read(&profile_path).unwrap()),
                },
                staging_ticket: Some(graph::PinnedFile {
                    path: stage_path.clone(),
                    sha256: digest(&fs::read(stage_path).unwrap()),
                }),
            },
        );
        let batch_path = case_root.join("control-ready.json");
        graph::export_ready(
            &parent_plan,
            &graph_bindings,
            std::slice::from_ref(&packet.task.id),
            &batch_path,
        )
        .unwrap();
        let batch = graph::PinnedFile {
            sha256: digest(&fs::read(&batch_path).unwrap()),
            path: batch_path,
        };
        let scratch = case_root.join("control-worker-scratch");
        fs::create_dir(&scratch).unwrap();
        let worker_stage = staging::Profile {
            parent: scratch,
            ..stage
        };
        let plan_path = case_root.join("control-plan.json");
        let bindings_path = case_root.join("control-bindings.json");
        let stage_path = case_root.join("control-worker-profile.json");
        let ticket_path = case_root.join("control-worker-ticket.json");
        save(&plan_path, &parent_plan);
        save(&bindings_path, &graph_bindings);
        save(&stage_path, &worker_stage);
        run(
            &executable,
            &[
                "prepare-worker".as_ref(),
                plan_path.as_os_str(),
                bindings_path.as_os_str(),
                batch.path.as_os_str(),
                batch.sha256.as_ref(),
                packet.task.id.as_ref(),
                stage_path.as_os_str(),
                ticket_path.as_os_str(),
                "--apply".as_ref(),
            ],
        );
        let raw = fs::read(&ticket_path).unwrap();
        let ticket: worker::Ticket = serde_json::from_slice(&raw).unwrap();
        run(
            &executable,
            &[
                "run-worker".as_ref(),
                plan_path.as_os_str(),
                bindings_path.as_os_str(),
                ticket_path.as_os_str(),
                digest(&raw).as_ref(),
                "--apply".as_ref(),
            ],
        );
        let directory = ticket.directory();
        let result: ReturnedResult =
            serde_json::from_slice(&fs::read(directory.join("result.json")).unwrap()).unwrap();
        let ack: retention::Acknowledgement = serde_json::from_slice(
            &fs::read(acks.join(format!("retained-{}-attempt-1.json", packet.task_key))).unwrap(),
        )
        .unwrap();
        retention::verify(&packet, &source, &profile, &admitted, &result, &ack).unwrap();
        assembly_asset = result.artifacts["assembly.json"].clone();
        let raw = fs::read(directory.join("packet/assembly.json")).unwrap();
        let derived: Assembly = serde_json::from_slice(&raw).unwrap();
        derived.check_version().unwrap();
        if intent_bound {
            assert_eq!(derived.schema, "reel.comic-issue-assembly.v2");
            assert_eq!(
                derived.composition.as_ref().unwrap().intent.sha256,
                intent_asset.as_ref().unwrap().sha256
            );
        }
        assert_eq!(derived.parts.len(), 2);
        assert!(!assets.values.contains_key(&assembly_asset.logical_id));
        upstream_refs.insert(
            assembly_asset.logical_id.clone(),
            reel_workflow::upstream::Reference {
                task_id: packet.task.id.clone(),
                artifact: reel_workflow::upstream::Selector::Output {
                    name: "assembly.json".into(),
                },
            },
        );
        parents.push(Parent {
            packet,
            source,
            profile,
            receipt: admitted,
            result,
            ack,
        });
    }
    let refs: BTreeMap<_, _> = parents
        .iter()
        .map(|p| {
            (
                p.packet.task.id.clone(),
                upstream::Parent {
                    packet: &p.packet,
                    result: &p.result,
                    source: &p.source,
                    admission: &p.profile,
                    receipt: &p.receipt,
                    acknowledgement: &p.ack,
                },
            )
        })
        .collect();
    let bindings = reel_workflow::upstream::Bindings {
        streaming: None,
        schema: "reel.workflow-upstream-inputs.v1".into(),
        inputs: upstream_refs,
    };
    let mut issue_inputs = BTreeMap::from([
        (master.logical_id.clone(), master.clone()),
        (assembly_asset.logical_id.clone(), assembly_asset.clone()),
    ]);
    for id in issue_paths.values() {
        issue_inputs.insert(id.clone(), assets.values[id].0.clone());
    }
    let issue_inputs: Vec<_> = issue_inputs.into_values().collect();
    let original_inputs: Vec<_> = issue_inputs
        .iter()
        .filter(|a| !bindings.inputs.contains_key(&a.logical_id))
        .cloned()
        .collect();
    let mut snapshot = assets.snapshot(&fixture, &original_inputs);
    snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
    snapshot.upstream = Some(upstream::Frozen::capture(&bindings, &refs));
    let binding_raw = bindings.canonical_bytes().unwrap();
    let task = Task {
        id: "issue-assemble".into(),
        native_inputs: Some(Fixture::declaration(&snapshot)),
        upstream_inputs: Some(Artifact {
            logical_id: "issue/upstream".into(),
            sha256: digest(&binding_raw),
            bytes: binding_raw.len() as u64,
            media_type: "application/json".into(),
        }),
        operation: Operation::DeterministicTool,
        tool: Some(ToolContract {
            auxiliary_executables: BTreeMap::new(),
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::ComicIssueAssembly,
            executable: tool.clone(),
            publication: master,
            edition_id: "es".into(),
            request: assembly_asset,
            source_bindings: issue_paths.clone(),
            part: None,
        }),
        language: Some("es".into()),
        prompt: String::new(),
        inputs: issue_inputs,
        depends_on: parents.iter().map(|p| p.packet.task.id.clone()).collect(),
        outputs: final_contracts,
    };
    let mut plan = parent_plan;
    plan.tasks.push(task);
    let packet = make_packet(
        &plan,
        "issue-assemble",
        parents
            .iter()
            .map(|p| {
                (
                    p.packet.task.id.clone(),
                    DependencyResult {
                        result_sha256: canonical_digest(&p.result).unwrap(),
                        artifacts: p.result.artifacts.clone(),
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    let source = NativeProfile {
        movie_verifier: None,
        snapshot,
        annex,
        source: pool,
    };
    save(&case_root.join("proposed-issue-packet.json"), &packet);
    save(&case_root.join("proposed-issue-source.json"), &source);
    save(
        &case_root.join("proposed-issue-admission.json"),
        &case.profile,
    );
    let prepared =
        upstream::Admission::prepare(&packet, &source, &case.profile, "attempt-1", refs).unwrap();
    let preview = prepared.preview().unwrap();
    assert!(preview.declared_objects + 4 <= preview.changed_path_limit);
    save(&case_root.join("issue-admission-preview.json"), &preview);
    let admitted = prepared.accept().unwrap();
    {
        use tig_core::tree::{NodeStore, Tree, TreeLimits};
        let mut nodes = tig_store::metadata::MetadataStore::open(
            &case.profile.metadata_root,
            &admitted.metadata_checkpoint,
        )
        .unwrap();
        let raw = nodes.get(&admitted.candidate).unwrap().unwrap();
        let descriptor: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(descriptor["schema"], "reel.workflow-input-projection.v2");
        let entries = Tree::from_root(admitted.input.root.clone())
            .entries(&mut nodes, TreeLimits::default())
            .unwrap()
            .0;
        let mut inputs = packet.task.inputs.iter().collect::<Vec<_>>();
        inputs.sort_by(|a, b| a.logical_id.cmp(&b.logical_id));
        for (index, a) in inputs.iter().enumerate() {
            let path = format!("inputs/{index:03}");
            assert_eq!(descriptor["input_paths"][&a.logical_id], path);
            assert_eq!(
                entries[&LogicalPath::parse(&path).unwrap()].sha256.as_str(),
                a.sha256
            );
        }
        assert_eq!(
            descriptor["input_paths"].as_object().unwrap().len(),
            inputs.len()
        );
    }
    for a in &packet.task.inputs {
        if !bindings.inputs.contains_key(&a.logical_id) && published.insert(a.sha256.clone()) {
            publish_input(&case_root, &source, &assets.values[&a.logical_id].1);
        }
    }
    // The helper's private graph controls use this original case root.
    let child_acks = case_root.join("issue-acks");
    fs::create_dir(&child_acks).unwrap();
    let profile = reel_workflow_native::handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source: clone_source(&source),
        admission: case.profile.clone(),
        binding: reel_workflow_native::handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: admitted.clone(),
            target: retained_target(),
        },
        runtime: Tool {
            path: executable.clone(),
            sha256: tool.sha256.clone(),
            bytes: tool.bytes,
        },
        acknowledgements: child_acks.clone(),
    };
    let packet_path = case_root.join("issue-packet.json");
    let profile_path = case_root.join("issue-profile.json");
    let journal = case_root.join("issue-journal");
    save(&packet_path, &packet);
    save(&profile_path, &profile);
    run(
        &executable,
        &[
            "create-handoff".as_ref(),
            packet_path.as_os_str(),
            profile_path.as_os_str(),
            journal.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let input_scratch = case_root.join("input-scratch");
    fs::create_dir(&input_scratch).unwrap();
    let stage = staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: input_scratch,
        maximum_bytes: 128 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 65536,
            minimum_free_basis_points: 0,
            max_object_bytes: 512 * 1024 * 1024,
            ..Default::default()
        },
    };
    let stage_ticket =
        staging::prepare(&packet, &source, &case.profile, &admitted, &stage).unwrap();
    let stage_raw = stage_ticket.canonical_bytes().unwrap();
    let stage_path = case_root.join("input-ticket.json");
    fs::write(&stage_path, &stage_raw).unwrap();
    continue_issue(Continuation {
        case_root,
        fixture_root,
        packet,
        profile,
        profile_path,
        plan,
        graph_bindings,
        stage,
        expect_close_refusal: false,
    });
}

struct Continuation {
    case_root: PathBuf,
    fixture_root: PathBuf,
    packet: Packet,
    profile: reel_workflow_native::handoff::Profile,
    profile_path: PathBuf,
    plan: Plan,
    graph_bindings: graph::Bindings,
    stage: staging::Profile,
    expect_close_refusal: bool,
}

/// A bounded recovery fixture for the recorded export refusal, not a new attempt.
#[test]
#[ignore = "Requires the preserved admitted synthetic issue root and its exact original frozen CLI; resumes original ticket/WorkRef only"]
fn admitted_issue_staging_recovery_preserves_original_close_refusal() {
    fn load<T: serde::de::DeserializeOwned>(path: &Path) -> T {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }
    let case_root = PathBuf::from(
        std::env::var_os("REEL_NATIVE_ISSUE_RECOVERY_ROOT")
            .expect("original recovery root required"),
    );
    assert!(case_root.is_absolute() && case_root.is_dir());
    if case_root.join("ready.json").exists() {
        let profile: reel_workflow_native::handoff::Profile =
            load(&case_root.join("issue-recovery-profile.json"));
        let packet: Packet = load(&case_root.join("issue-packet.json"));
        verify_pending_close_refusal(&case_root, &packet, &profile);
        return;
    }
    assert!(!case_root.join("ready.json").exists());
    assert!(!case_root.join("worker-ticket.json").exists());
    let packet: Packet = load(&case_root.join("issue-packet.json"));
    assert_eq!(packet.project, "synthetic-comic");
    assert_eq!(packet.task.id, "issue-assemble");
    let mut profile: reel_workflow_native::handoff::Profile =
        load(&case_root.join("issue-profile.json"));
    assert!(profile.admission.index_root.starts_with(&case_root));
    let fixture_root = profile.source.snapshot.repository.clone();
    let original_runtime = PathBuf::from(
        std::env::var_os("REEL_NATIVE_ISSUE_ORIGINAL_RUNTIME")
            .expect("exact original frozen runtime required"),
    );
    let runtime_raw = fs::read(&original_runtime).unwrap();
    assert_eq!(digest(&runtime_raw), profile.runtime.sha256);
    assert_eq!(runtime_raw.len() as u64, profile.runtime.bytes);
    profile.runtime.path = original_runtime.clone();
    let profile_path = case_root.join("issue-recovery-profile.json");
    assert!(!profile_path.exists());
    save(&profile_path, &profile);
    let observed = reel_workflow::handoff::Journal::open(&case_root.join("issue-journal")).unwrap();
    assert_eq!(observed.state(), reel_workflow::handoff::State::Waiting);
    assert_eq!(observed.ticket().packet, packet);
    assert!(observed.result().is_none());
    drop(
        admission::Prepared::open(
            &packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission.attempt_id,
        )
        .unwrap()
        .reopen(&profile.binding.admission)
        .unwrap(),
    );
    let mut graph_bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::new(),
    };
    let frozen = profile.source.snapshot.upstream.as_ref().unwrap();
    let mut tasks = Vec::new();
    for (id, evidence) in &frozen.parents {
        let mut parent: reel_workflow_native::handoff::Profile =
            load(&case_root.join(format!("{id}-profile.json")));
        assert_eq!(parent.runtime.sha256, profile.runtime.sha256);
        parent.runtime.path = original_runtime.clone();
        let path = case_root.join(format!("{id}-recovery-profile.json"));
        assert!(!path.exists());
        save(&path, &parent);
        graph_bindings.tasks.insert(
            id.clone(),
            graph::Binding {
                journal: case_root.join(format!("{id}-journal")),
                profile: graph::PinnedFile {
                    path: path.clone(),
                    sha256: digest(&fs::read(path).unwrap()),
                },
                staging_ticket: None,
            },
        );
        tasks.push(evidence.packet.task.clone());
    }
    tasks.push(packet.task.clone());
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: packet.project.clone(),
        source_commit: packet.source_commit.clone(),
        tasks,
    };
    let ticket_raw = fs::read(case_root.join("input-ticket.json")).unwrap();
    let ticket_json: serde_json::Value = serde_json::from_slice(&ticket_raw).unwrap();
    let original: admission::Receipt =
        serde_json::from_value(ticket_json["admission"].clone()).unwrap();
    assert_eq!(
        original.canonical_bytes().unwrap(),
        profile.binding.admission.canonical_bytes().unwrap()
    );
    let stage: staging::Profile = serde_json::from_value(ticket_json["profile"].clone()).unwrap();
    if let Some(evidence) = std::env::var_os("REEL_NATIVE_ISSUE_EVIDENCE_DIRECTORY") {
        let evidence = PathBuf::from(evidence);
        assert!(evidence.is_absolute() && evidence.is_dir());
        save(
            &evidence.join("original-recovery.json"),
            &json!({"root":case_root,"task_key":packet.task_key,
            "admission":profile.binding.admission,"input_ticket_sha256":digest(&ticket_raw),"runtime":profile.runtime,
            "new_admission":false,"new_attempt":false,"claims_released":false}),
        );
    }
    continue_issue(Continuation {
        case_root,
        fixture_root,
        packet,
        profile,
        profile_path,
        plan,
        graph_bindings,
        stage,
        expect_close_refusal: true,
    });
}

fn continue_issue(state: Continuation) {
    let Continuation {
        case_root,
        fixture_root,
        packet,
        mut profile,
        profile_path,
        plan,
        mut graph_bindings,
        stage,
        expect_close_refusal,
    } = state;
    let admission = profile.admission.clone();
    let admitted = profile.binding.admission.clone();
    let executable = profile.runtime.path.clone();
    let child_acks = profile.acknowledgements.clone();
    let journal = case_root.join("issue-journal");
    let packet_path = case_root.join("issue-packet.json");
    let stage_path = case_root.join("input-ticket.json");
    let stage_raw = fs::read(&stage_path).unwrap();
    graph_bindings.tasks.insert(
        packet.task.id.clone(),
        graph::Binding {
            journal: journal.clone(),
            profile: graph::PinnedFile {
                path: profile_path.clone(),
                sha256: digest(&fs::read(&profile_path).unwrap()),
            },
            staging_ticket: Some(graph::PinnedFile {
                path: stage_path,
                sha256: digest(&stage_raw),
            }),
        },
    );
    let batch_path = case_root.join("ready.json");
    graph::export_ready(
        &plan,
        &graph_bindings,
        std::slice::from_ref(&packet.task.id),
        &batch_path,
    )
    .unwrap();
    let batch = graph::PinnedFile {
        path: batch_path,
        sha256: digest(&fs::read(case_root.join("ready.json")).unwrap()),
    };
    let worker_root = case_root.join("worker-scratch");
    fs::create_dir(&worker_root).unwrap();
    let worker_stage = staging::Profile {
        parent: worker_root,
        ..stage.clone()
    };
    let plan_path = case_root.join("workflow-plan.json");
    let bindings_path = case_root.join("workflow-bindings.json");
    let stage_path = case_root.join("worker-profile.json");
    let worker_path = case_root.join("worker-ticket.json");
    save(&plan_path, &plan);
    save(&bindings_path, &graph_bindings);
    save(&stage_path, &worker_stage);
    run(
        &executable,
        &[
            "prepare-worker".as_ref(),
            plan_path.as_os_str(),
            bindings_path.as_os_str(),
            batch.path.as_os_str(),
            batch.sha256.as_ref(),
            packet.task.id.as_ref(),
            stage_path.as_os_str(),
            worker_path.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let worker_raw = fs::read(&worker_path).unwrap();
    let worker_pin = digest(&worker_raw);
    let worker_ticket: worker::Ticket = serde_json::from_slice(&worker_raw).unwrap();
    if expect_close_refusal {
        verify_pending_close_refusal(&case_root, &packet, &profile);
        return;
    }
    run(
        &executable,
        &[
            "run-worker".as_ref(),
            plan_path.as_os_str(),
            bindings_path.as_os_str(),
            worker_path.as_os_str(),
            worker_pin.as_ref(),
            "--apply".as_ref(),
        ],
    );
    let worker_packet = worker_ticket.directory().join("packet");
    let result: ReturnedResult =
        serde_json::from_slice(&fs::read(worker_ticket.directory().join("result.json")).unwrap())
            .unwrap();
    let assembled: reel_publication::comic_issue_source::AssemblyReceipt =
        serde_json::from_slice(&fs::read(worker_packet.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(assembled.page_order, ["page01", "page02"]);
    assert_eq!(assembled.parts.len(), 2);
    if packet
        .task
        .tool
        .as_ref()
        .unwrap()
        .source_bindings
        .contains_key("intent.json")
    {
        let captured: Assembly =
            serde_json::from_slice(&fs::read(worker_packet.join("assembly.json")).unwrap())
                .unwrap();
        captured.check_version().unwrap();
        assert_eq!(captured.schema, "reel.comic-issue-assembly.v2");
        let pin = &captured.composition.as_ref().unwrap().intent;
        assert_eq!(pin.path, "composition-intent.json");
        let raw = fs::read(worker_packet.join(&pin.path)).unwrap();
        assert_eq!(digest(&raw), pin.sha256);
        assert_eq!(raw.len() as u64, pin.bytes);
    }
    assert!(
        !assembled.human_approval
            && !assembled.print_ready
            && !assembled.retained_owner_acknowledged
    );
    let ack: retention::Acknowledgement = serde_json::from_slice(
        &fs::read(child_acks.join(format!("retained-{}-attempt-1.json", packet.task_key))).unwrap(),
    )
    .unwrap();
    // Synthetic loss of source-actor coverage is not production slot retirement.
    {
        use tig_store::{
            metadata::MetadataStore,
            source_claims::{
                Selection,
                protection::{Index, work},
            },
        };
        let mut nodes =
            MetadataStore::open(&admission.metadata_root, &admitted.metadata_checkpoint).unwrap();
        let mut index = Index::open(&admission.index_root, &admitted.index_checkpoint).unwrap();
        let actor = index.actor(&mut nodes, &admission.scope.actor).unwrap();
        let entries = tig_core::tree::Tree::from_root(actor.selection.root.clone())
            .entries(&mut nodes, tig_core::tree::TreeLimits::default())
            .unwrap()
            .0;
        let empty = Selection {
            root: tig_core::tree::Tree::empty(&mut nodes)
                .unwrap()
                .root()
                .clone(),
            context: actor.selection.context,
        };
        let update = tig_store::source_claims::protection::Update {
            operation: tig_store::Key::parse("fixture-remove-issue-job-coverage").unwrap(),
            actor: admission.scope.actor.clone(),
            owner: admission.scope.owner.clone(),
            selection: empty,
            expected_revision: actor.revision,
            changes: entries.keys().map(|p| (p.clone(), None)).collect(),
            ready: true,
        };
        // Same bounded synthetic update protocol used by retained-history fixtures.
        let reference = work::stage(&mut index, &update, None, &mut nodes).unwrap();
        work::execute(&mut index, &reference, &mut nodes).unwrap();
    }
    let bare = case_root.join("issue-history.git");
    git(
        &case_root,
        &[
            "clone",
            "--bare",
            fixture_root.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    profile.source.snapshot.repository = bare;
    save(&profile_path, &profile);
    let historical = run(
        &executable,
        &[
            "resume-handoff".as_ref(),
            journal.as_os_str(),
            profile_path.as_os_str(),
            "-".as_ref(),
            "--apply".as_ref(),
        ],
    );
    retention::verify(
        &packet,
        &profile.source,
        &profile.admission,
        &admitted,
        &result,
        &ack,
    )
    .unwrap();
    let export_root = case_root.join("export-scratch");
    fs::create_dir(&export_root).unwrap();
    let export_stage = staging::Profile {
        parent: export_root,
        ..stage
    };
    let export_source = case_root.join("export-source.json");
    let export_admission = case_root.join("export-admission.json");
    let export_receipt = case_root.join("export-admission-receipt.json");
    let result_path = case_root.join("issue-result.json");
    let ack_path = case_root.join("issue-retained.json");
    let export_profile = case_root.join("export-profile.json");
    let export_ticket = case_root.join("export-ticket.json");
    save(&export_source, &profile.source);
    save(&export_admission, &profile.admission);
    save(&export_receipt, &admitted);
    save(&result_path, &result);
    save(&ack_path, &ack);
    save(&export_profile, &export_stage);
    run(
        &executable,
        &[
            "prepare-comic-export".as_ref(),
            packet_path.as_os_str(),
            export_source.as_os_str(),
            export_admission.as_os_str(),
            export_receipt.as_os_str(),
            result_path.as_os_str(),
            ack_path.as_os_str(),
            export_profile.as_os_str(),
            export_ticket.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let export_raw = fs::read(&export_ticket).unwrap();
    let export_pin = digest(&export_raw);
    let ticket: reel_workflow_native::comic_export::Ticket =
        serde_json::from_slice(&export_raw).unwrap();
    run(
        &executable,
        &[
            "export-comic".as_ref(),
            packet_path.as_os_str(),
            export_source.as_os_str(),
            export_admission.as_os_str(),
            export_ticket.as_os_str(),
            export_pin.as_ref(),
            "--apply".as_ref(),
        ],
    );
    let exported = ticket.packet_directory();
    for (name, a) in &result.artifacts {
        assert_eq!(digest(&fs::read(exported.join(name)).unwrap()), a.sha256);
    }
    let rebuilt = case_root.join("rebuilt-issue");
    run(
        &executable,
        &[
            "comic-issue-assemble".as_ref(),
            exported.join("assembly.json").as_os_str(),
            rebuilt.as_os_str(),
        ],
    );
    assert_eq!(
        fs::read(rebuilt.join("reading.pdf")).unwrap(),
        fs::read(exported.join("reading.pdf")).unwrap()
    );
    let restored = case_root.join("restored-opening");
    let part_root = exported.join("parts/opening");
    run(
        &executable,
        &[
            "comic-restore".as_ref(),
            part_root.as_os_str(),
            restored.as_os_str(),
            assembled.parts["opening"].sha256.as_ref(),
        ],
    );
    assert!(restored.join("pages/page01.json").is_file());
    let mut request: Request =
        serde_json::from_slice(&fs::read(restored.join("request.json")).unwrap()).unwrap();
    assert_eq!(request.part_id.as_deref(), Some("opening"));
    let page_path = restored.join(&request.pages[0].path);
    let mut page: PageSource = serde_json::from_slice(&fs::read(&page_path).unwrap()).unwrap();
    page.utterances[0].lines = vec!["Mira la luz.".into()];
    let changed = serde_json::to_vec_pretty(&page).unwrap();
    fs::write(page_path, &changed).unwrap();
    request.pages[0].sha256 = digest(&changed);
    request.pages[0].bytes = changed.len() as u64;
    save(&restored.join("request.json"), &request);
    let edited = case_root.join("edited-opening");
    run(
        &executable,
        &[
            "comic-render".as_ref(),
            restored.join("request.json").as_os_str(),
            edited.as_os_str(),
        ],
    );
    assert_eq!(
        fs::read(edited.join("page01-clean.png")).unwrap(),
        fs::read(part_root.join("page01-clean.png")).unwrap()
    );
    assert_ne!(
        fs::read(edited.join("page01-reading.png")).unwrap(),
        fs::read(part_root.join("page01-reading.png")).unwrap()
    );
    if let Some(evidence) = std::env::var_os("REEL_NATIVE_ISSUE_EVIDENCE_DIRECTORY") {
        let evidence = PathBuf::from(evidence);
        assert!(evidence.is_absolute() && evidence.is_dir());
        save(
            &evidence.join("locations.json"),
            &json!({"root":case_root,"fixture":fixture_root,"packet":packet_path,"profile":profile_path,"worker":worker_ticket.directory(),"exported":exported,"rebuilt":rebuilt,"restored":restored,"edited":edited}),
        );
        fs::write(evidence.join("historical.json"), historical).unwrap();
    }
}

fn verify_pending_close_refusal(
    root: &Path,
    packet: &Packet,
    profile: &reel_workflow_native::handoff::Profile,
) {
    use tig_store::source_claims::protection::Index;
    assert_eq!(packet.project, "synthetic-comic");
    assert!(profile.admission.index_root.starts_with(root));
    let receipt = &profile.binding.admission;
    let runtime_raw = fs::read(&profile.runtime.path).unwrap();
    assert_eq!(digest(&runtime_raw), profile.runtime.sha256);
    assert_eq!(runtime_raw.len() as u64, profile.runtime.bytes);
    let original = reel_workflow::handoff::Journal::open(&root.join("issue-journal")).unwrap();
    assert_eq!(
        original.ticket().native_binding,
        profile.binding.artifact().unwrap()
    );
    assert_eq!(
        original.ticket().lifecycle_tool.sha256,
        profile.runtime.sha256
    );
    assert_eq!(original.ticket().packet, *packet);
    let before = Index::open(&profile.admission.index_root, &receipt.index_checkpoint)
        .unwrap()
        .checkpoint();
    drop(
        admission::Prepared::open(
            packet,
            &profile.source,
            &profile.admission,
            &receipt.attempt_id,
        )
        .unwrap()
        .reopen(receipt)
        .unwrap(),
    );
    let ticket = root.join("worker-ticket.json");
    let raw = fs::read(&ticket).unwrap();
    let output = Command::new(&profile.runtime.path)
        .args([
            "run-worker".as_ref(),
            root.join("workflow-plan.json").as_os_str(),
            root.join("workflow-bindings.json").as_os_str(),
            ticket.as_os_str(),
            digest(&raw).as_ref(),
            "--apply".as_ref(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["kind"], "budget");
    assert_eq!(
        response["error"]["code"],
        "handoff-ingest:response-job-close-path-limit"
    );
    assert_eq!(
        Index::open(&profile.admission.index_root, &receipt.index_checkpoint)
            .unwrap()
            .checkpoint(),
        before
    );
    drop(
        admission::Prepared::open(
            packet,
            &profile.source,
            &profile.admission,
            &receipt.attempt_id,
        )
        .unwrap()
        .reopen(receipt)
        .unwrap(),
    );
    let journal = reel_workflow::handoff::Journal::open(&root.join("issue-journal")).unwrap();
    assert_eq!(
        journal.state(),
        reel_workflow::handoff::State::ImportPending
    );
    assert!(journal.result().is_some());
    assert!(
        !profile
            .acknowledgements
            .join(format!(
                "retained-{}-{}.json",
                packet.task_key, receipt.attempt_id
            ))
            .exists()
    );
    if let Some(directory) = std::env::var_os("REEL_NATIVE_ISSUE_EVIDENCE_DIRECTORY") {
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        save(
            &directory.join("guarded-original-refusal.json"),
            &json!({"root":root,"work":receipt.work,"activation":receipt.activation,
            "task_key":packet.task_key,"attempt":receipt.attempt_id,"state":"import-pending","task_completed":false,
            "original_claims_reopened":true,"index_unchanged":true,"scratch_preserved":true,"response":response}),
        );
    }
}
