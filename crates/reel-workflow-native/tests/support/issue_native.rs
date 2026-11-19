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
    let descriptor: serde_json::Value = serde_json::from_slice(
        &fs::read(std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("native fixture required"))
            .unwrap(),
    )
    .unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(descriptor["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(descriptor["annex"].clone()).unwrap();
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native"));
    let worker_raw = fs::read(&executable).unwrap();
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
    let mut upstream_refs = BTreeMap::new();
    for part in &plan.parts {
        let request = Request {
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
            issue_paths.insert(
                format!("parts/{}/sources/{path}", part.id),
                asset.logical_id.clone(),
            );
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
            issue_paths.insert(format!("parts/{}/{name}", part.id), a.logical_id.clone());
            upstream_refs.insert(
                a.logical_id.clone(),
                reel_workflow::upstream::Reference {
                    task_id: part.id.clone(),
                    artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
                },
            );
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
    let assembly = Assembly {
        schema: "reel.comic-issue-assembly.v1".into(),
        plan: assets.pin("issue-plan.json"),
        parts: receipt_pins,
        limits: Limits {
            input_bytes: 32 * 1024 * 1024,
            output_bytes: 32 * 1024 * 1024,
        },
    };
    let assembly_raw = serde_json::to_vec_pretty(&assembly).unwrap();
    let assembly_asset = assets.put(
        "comic/assembly",
        "assembly-request.json",
        "application/json",
        assembly_raw.clone(),
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
    let parent_plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic-comic".into(),
        source_commit: fixture.profile.commit.clone(),
        tasks: part_data.iter().map(|p| p.0.clone()).collect(),
    };
    let mut published = BTreeSet::new();
    for (index, (task, receipt, worker_receipt, files)) in part_data.iter().enumerate() {
        let packet = make_packet(&parent_plan, &task.id, BTreeMap::new()).unwrap();
        let source = NativeProfile {
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
        snapshot,
        annex,
        source: pool,
    };
    let admitted = upstream::Admission::prepare(&packet, &source, &case.profile, "attempt-1", refs)
        .unwrap()
        .accept()
        .unwrap();
    for a in &packet.task.inputs {
        if !bindings.inputs.contains_key(&a.logical_id) && published.insert(a.sha256.clone()) {
            publish_input(&case_root, &source, &assets.values[&a.logical_id].1);
        }
    }
    // The helper's private graph controls use this original case root.
    let child_acks = case_root.join("issue-acks");
    fs::create_dir(&child_acks).unwrap();
    let mut profile = reel_workflow_native::handoff::Profile {
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
            MetadataStore::open(&case.profile.metadata_root, &admitted.metadata_checkpoint)
                .unwrap();
        let mut index = Index::open(&case.profile.index_root, &admitted.index_checkpoint).unwrap();
        let actor = index.actor(&mut nodes, &case.profile.scope.actor).unwrap();
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
            actor: case.profile.scope.actor.clone(),
            owner: case.profile.scope.owner.clone(),
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
