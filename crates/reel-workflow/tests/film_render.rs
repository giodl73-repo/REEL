use reel_workflow::{
    film_render::{self, Limits, Request},
    publication::InputReader,
    *,
};
use std::collections::BTreeMap;
struct Fixture {
    packet: Packet,
    raw: BTreeMap<String, Vec<u8>>,
}
fn artifact(id: &str, raw: &[u8], media: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: media.into(),
    }
}
impl InputReader for Fixture {
    fn read(&mut self, artifact: &Artifact, _: u64) -> Result<Vec<u8>> {
        self.raw.get(&artifact.logical_id).cloned().ok_or(Error {
            kind: ErrorKind::Missing,
            code: "fixture-input".into(),
        })
    }
}
impl Fixture {
    fn new() -> Self {
        let job = artifact("film/job", b"{}", "application/json");
        let production = artifact(
            "film/production",
            br#"{"production":1}"#,
            "application/json",
        );
        let request = Request {
            schema: "reel.workflow-film-render.v1".into(),
            streaming: None,
            job_path: "controls/job.json".into(),
            production_path: "controls/production.json".into(),
            output_paths: [
                ("execution-request", "render-request.json"),
                ("execution-result", "render-result.json"),
                ("delivery-receipt", "delivery/receipt.json"),
                ("review", "delivery/review.mp4"),
                ("frame", "delivery/motion/frames/0001.png"),
            ]
            .into_iter()
            .map(|(a, b)| (a.into(), b.into()))
            .collect(),
            limits: Limits {
                timeout_seconds: 120,
                memory_bytes: 1024 * 1024 * 1024,
                processes: 8,
                workspace_bytes: 128 * 1024 * 1024,
                output_bytes: 32 * 1024 * 1024,
                frames: 48,
                samples: 96000,
                raw_rgb_bytes: 64 * 1024 * 1024,
            },
        };
        let raw = serde_json::to_vec(&request).unwrap();
        let request_asset = artifact("film/request", &raw, "application/json");
        let task = Task {
            id: "movie".into(),
            native_inputs: None,
            upstream_inputs: None,
            operation: Operation::DeterministicTool,
            tool: Some(ToolContract {
                schema: "reel.workflow-tool.v1".into(),
                capability: ToolCapability::FilmRender,
                executable: artifact("tools/native", b"runtime", "application/octet-stream"),
                auxiliary_executables: BTreeMap::from([
                    (
                        "ffmpeg".into(),
                        artifact("tools/ffmpeg", b"ffmpeg", "application/octet-stream"),
                    ),
                    (
                        "ffprobe".into(),
                        artifact("tools/ffprobe", b"ffprobe", "application/octet-stream"),
                    ),
                ]),
                publication: job.clone(),
                request: request_asset.clone(),
                edition_id: "edition-es".into(),
                source_bindings: BTreeMap::from([
                    ("controls/job.json".into(), job.logical_id.clone()),
                    (
                        "controls/production.json".into(),
                        production.logical_id.clone(),
                    ),
                ]),
                part: None,
            }),
            language: Some("es".into()),
            prompt: String::new(),
            inputs: vec![job.clone(), production.clone(), request_asset],
            depends_on: vec![],
            outputs: request
                .output_paths
                .iter()
                .map(|(name, path)| {
                    (
                        name.clone(),
                        OutputContract {
                            media_type: film_render::media_type(path).unwrap().into(),
                            max_bytes: 4096,
                            required: true,
                        },
                    )
                })
                .collect(),
        };
        let plan = Plan {
            schema: PLAN_SCHEMA.into(),
            task_snapshots: Default::default(),
            project: "film".into(),
            source_commit: "a".repeat(40),
            tasks: vec![task],
        };
        Self {
            packet: make_packet(&plan, "movie", BTreeMap::new()).unwrap(),
            raw: BTreeMap::from([
                (job.logical_id, b"{}".to_vec()),
                (production.logical_id, br#"{"production":1}"#.to_vec()),
                ("film/request".into(), raw),
            ]),
        }
    }
    fn repin(&mut self) {
        self.packet = make_packet(
            &Plan {
                schema: PLAN_SCHEMA.into(),
                task_snapshots: Default::default(),
                project: self.packet.project.clone(),
                source_commit: self.packet.source_commit.clone(),
                tasks: vec![self.packet.task.clone()],
            },
            "movie",
            BTreeMap::new(),
        )
        .unwrap();
    }
    fn change(&mut self, change: impl FnOnce(&mut Request)) {
        let mut request: Request = decode(&self.raw["film/request"]).unwrap();
        change(&mut request);
        let raw = serde_json::to_vec(&request).unwrap();
        let next = artifact("film/request", &raw, "application/json");
        self.packet.task.tool.as_mut().unwrap().request = next.clone();
        *self
            .packet
            .task
            .inputs
            .iter_mut()
            .find(|a| a.logical_id == "film/request")
            .unwrap() = next;
        self.raw.insert("film/request".into(), raw);
        self.repin();
    }
}
#[test]
fn exact_movie_contract_preserves_nested_paths_and_separate_receipts() {
    let mut f = Fixture::new();
    let packet = f.packet.clone();
    let bound = film_render::bind(&packet, &mut f).unwrap();
    assert_eq!(bound.aliases().len(), 3);
    assert_eq!(bound.job().logical_id, "film/job");
    let files = bound
        .request()
        .output_paths
        .values()
        .map(|p| (p.clone(), b"candidate".to_vec()))
        .collect();
    let receipt = film_render::capture_outputs(&packet, &bound, &files).unwrap();
    assert_eq!(
        receipt.output_paths["frame"],
        "delivery/motion/frames/0001.png"
    );
    assert_eq!(receipt.outputs["review"].media_type, "video/mp4");
    assert!(!receipt.human_approval);
    assert!(!receipt.retained_owner_acknowledged);
    assert_ne!(receipt.output_paths["delivery-receipt"], "receipt.json");
}

#[test]
fn streamed_content_pins_preserve_receipt_bytes_and_enforce_closed_output_contract() {
    let mut fixture = Fixture::new();
    let packet = fixture.packet.clone();
    let bound = film_render::bind(&packet, &mut fixture).unwrap();
    let files: BTreeMap<_, _> = bound
        .request()
        .output_paths
        .values()
        .map(|p| (p.clone(), b"candidate".to_vec()))
        .collect();
    let receipt = film_render::capture_outputs(&packet, &bound, &files).unwrap();
    let pins: BTreeMap<_, _> = files
        .iter()
        .map(|(p, raw)| {
            (
                p.clone(),
                film_render::OutputContent {
                    sha256: digest(raw),
                    bytes: raw.len() as u64,
                },
            )
        })
        .collect();
    let streamed = film_render::capture_output_pins(&packet, &bound, &pins).unwrap();
    assert_eq!(
        serde_json::to_vec(&receipt).unwrap(),
        serde_json::to_vec(&streamed).unwrap()
    );
    let path = pins.keys().next().unwrap().clone();
    let mut bad = pins.clone();
    bad.remove(&path);
    assert!(film_render::capture_output_pins(&packet, &bound, &bad).is_err());
    let mut bad = pins.clone();
    bad.get_mut(&path).unwrap().sha256 = "z".repeat(64);
    assert!(film_render::capture_output_pins(&packet, &bound, &bad).is_err());
    let mut bad = pins;
    bad.get_mut(&path).unwrap().bytes = u64::MAX;
    assert_eq!(
        film_render::capture_output_pins(&packet, &bound, &bad)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
}
#[test]
fn versioned_streaming_intent_accepts_e7_budgets_without_upgrading_native_execution() {
    let mut f = Fixture::new();
    let original = f.packet.clone();
    let legacy: Request = decode(&f.raw["film/request"]).unwrap();
    assert!(
        serde_json::to_value(&legacy)
            .unwrap()
            .get("streaming")
            .is_none()
    );
    f.change(|r| {
        r.schema = "reel.workflow-film-render.v2".into();
        r.streaming = Some(film_render::Streaming {
            schema: "reel.workflow-film-streaming.v1".into(),
            input_bytes: 7_936_104_893,
            input_object_bytes: 4 * 1024 * 1024 * 1024,
            verify_millis: 300_000,
        });
        r.limits.timeout_seconds = 18_000;
        r.limits.workspace_bytes = 32 * 1024 * 1024 * 1024;
        // E7's three output ceilings plus two explicit workflow execution controls.
        r.limits.output_bytes = 9_663_741_952 + 2 * 65_536;
        r.limits.raw_rgb_bytes = 1024 * 1024 * 1024 * 1024;
        r.output_paths.remove("frame");
        r.output_paths
            .insert("master".into(), "delivery/master.mkv".into());
    });
    f.packet.task.outputs.remove("frame");
    for (name, ceiling) in [
        ("master", 8 * 1024 * 1024 * 1024),
        ("review", 1024 * 1024 * 1024),
        ("delivery-receipt", 65_536),
        ("execution-request", 65_536),
        ("execution-result", 65_536),
    ] {
        let path: Request = decode(&f.raw["film/request"]).unwrap();
        f.packet.task.outputs.insert(
            name.into(),
            OutputContract {
                media_type: film_render::media_type(&path.output_paths[name])
                    .unwrap()
                    .into(),
                max_bytes: ceiling,
                required: true,
            },
        );
    }
    f.repin();
    let p = f.packet.clone();
    assert_ne!(p.task_key, original.task_key);
    let bound = film_render::bind(&p, &mut f).unwrap();
    assert_eq!(bound.request().limits.timeout_seconds, 18_000);
    assert_eq!(
        bound.require_native_v1().unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert!(bound.request().limits.validate().is_err());
    let pins = bound
        .request()
        .output_paths
        .iter()
        .map(|(name, path)| {
            (
                path.clone(),
                film_render::OutputContent {
                    sha256: digest(b"synthetic unverified declared content"),
                    bytes: p.task.outputs[name].max_bytes,
                },
            )
        })
        .collect();
    // Identity metadata only: no multi-GB media is allocated or authenticated here.
    let receipt = film_render::capture_output_pins(&p, &bound, &pins).unwrap();
    assert_eq!(receipt.outputs["master"].bytes, 8 * 1024 * 1024 * 1024);
    // Legacy request bytes and the resulting packet stay unchanged when decoded.
    assert_eq!(
        serde_json::to_vec(&legacy).unwrap(),
        Fixture::new().raw["film/request"]
    );
    let mut old = Fixture::new();
    film_render::bind(&original, &mut old)
        .unwrap()
        .require_native_v1()
        .unwrap();
}

#[test]
fn streaming_envelope_cannot_be_omitted_downgraded_or_used_to_bypass_legacy_limits() {
    for mutation in [
        "missing",
        "legacy-with-envelope",
        "bad-stream-version",
        "zero-input",
        "object-over-total",
        "over-time",
    ] {
        let mut f = Fixture::new();
        f.change(|r| {
            r.schema = "reel.workflow-film-render.v2".into();
            r.streaming = Some(film_render::Streaming {
                schema: "reel.workflow-film-streaming.v1".into(),
                input_bytes: 1024 * 1024,
                input_object_bytes: 1024 * 1024,
                verify_millis: 30_000,
            });
            match mutation {
                "missing" => r.streaming = None,
                "legacy-with-envelope" => r.schema = "reel.workflow-film-render.v1".into(),
                "bad-stream-version" => r.streaming.as_mut().unwrap().schema = "future".into(),
                "zero-input" => r.streaming.as_mut().unwrap().input_object_bytes = 0,
                "object-over-total" => r.streaming.as_mut().unwrap().input_object_bytes += 1,
                _ => r.limits.timeout_seconds = 18_001,
            }
        });
        let p = f.packet.clone();
        assert!(film_render::bind(&p, &mut f).is_err(), "{mutation}");
    }
    let mut old = Fixture::new();
    old.change(|r| r.limits.timeout_seconds = 18_000);
    let p = old.packet.clone();
    assert_eq!(
        film_render::bind(&p, &mut old).unwrap_err().kind,
        ErrorKind::Budget
    );
}

#[test]
fn streaming_intent_enforces_input_objects_total_and_declared_output_ceilings() {
    for mutation in ["object", "total", "outputs"] {
        let mut f = Fixture::new();
        f.change(|r| {
            r.schema = "reel.workflow-film-render.v2".into();
            r.streaming = Some(film_render::Streaming {
                schema: "reel.workflow-film-streaming.v1".into(),
                input_bytes: 1024 * 1024,
                input_object_bytes: 1024 * 1024,
                verify_millis: 30_000,
            });
            if mutation == "object" {
                r.streaming.as_mut().unwrap().input_object_bytes = 1;
            }
            if mutation == "outputs" {
                r.limits.output_bytes = 1;
            }
        });
        if mutation == "total" {
            let mut task = f.packet.task.clone();
            task.inputs.extend((0..2).map(|n| Artifact {
                logical_id: format!("media/{n}"),
                sha256: digest(b"synthetic declared media"),
                bytes: 600 * 1024,
                media_type: "video/mp4".into(),
            }));
            f.packet = make_packet(
                &Plan {
                    schema: PLAN_SCHEMA.into(),
                    project: f.packet.project.clone(),
                    source_commit: f.packet.source_commit.clone(),
                    task_snapshots: BTreeMap::new(),
                    tasks: vec![task],
                },
                "movie",
                BTreeMap::new(),
            )
            .unwrap();
        }
        let p = f.packet.clone();
        let error = film_render::bind(&p, &mut f).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Budget, "{mutation}");
        assert_eq!(
            error.code,
            match mutation {
                "object" => "film-streaming-input-object",
                "total" => "film-streaming-input-total",
                _ => "film-streaming-output-ceilings",
            }
        );
    }
}

#[test]
fn cli_checks_large_intent_using_only_named_controls_without_opening_declared_media() {
    let mut f = Fixture::new();
    f.change(|r| {
        r.schema = "reel.workflow-film-render.v2".into();
        r.streaming = Some(film_render::Streaming {
            schema: "reel.workflow-film-streaming.v1".into(),
            input_bytes: 8 * 1024 * 1024 * 1024,
            input_object_bytes: 4 * 1024 * 1024 * 1024,
            verify_millis: 300_000,
        });
        r.limits.timeout_seconds = 18000;
        r.limits.workspace_bytes = 32 * 1024 * 1024 * 1024;
    });
    f.packet.task.inputs.push(Artifact {
        logical_id: "film/media".into(),
        sha256: digest(b"synthetic unverified media declaration"),
        bytes: 3 * 1024 * 1024 * 1024,
        media_type: "video/x-matroska".into(),
    });
    f.packet
        .task
        .tool
        .as_mut()
        .unwrap()
        .source_bindings
        .insert("media/master.mkv".into(), "film/media".into());
    f.repin();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let packet_path = root.join("packet.json");
    let bindings_path = root.join("bindings.json");
    std::fs::write(&packet_path, serde_json::to_vec(&f.packet).unwrap()).unwrap();
    let mut bindings = BTreeMap::new();
    for (i, (id, raw)) in f.raw.iter().enumerate() {
        let path = root.join(format!("control-{i}.json"));
        std::fs::write(&path, raw).unwrap();
        bindings.insert(id.clone(), path);
    }
    std::fs::write(&bindings_path, serde_json::to_vec(&bindings).unwrap()).unwrap();
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg("check-film-intent")
            .arg(&packet_path)
            .arg(&bindings_path)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["declared_inputs"], 4);
    assert_eq!(report["limits"]["timeout_seconds"], 18000);
    for field in [
        "media_bytes_verified",
        "tool_executed",
        "snapshot_binding_verified",
        "production_admitted",
        "native_v2_execution_qualified",
    ] {
        assert_eq!(report[field], false);
    }
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("film/media")
    );
    std::fs::write(&bindings["film/request"], b"corrupt control").unwrap();
    assert!(!run().status.success());
    assert!(!root.join("media").exists());
}

#[test]
fn render_limits_enter_task_identity() {
    let mut f = Fixture::new();
    let old = f.packet.task_key.clone();
    f.change(|r| r.limits.frames = 96);
    assert_ne!(old, f.packet.task_key);
    let p = f.packet.clone();
    film_render::bind(&p, &mut f).unwrap();
    f.change(|r| r.limits.timeout_seconds = 0);
    let p = f.packet.clone();
    assert_eq!(
        film_render::bind(&p, &mut f).unwrap_err().kind,
        ErrorKind::Budget
    );
}
#[test]
fn unsafe_paths_aliases_wrong_evidence_and_media_refuse_before_production() {
    for value in [
        "../escape.mp4",
        "delivery/aux/file.mp4",
        "delivery/Review.mp4.",
        "delivery/receipt.json",
        "delivery\\review.mp4",
        "/absolute.mp4",
    ] {
        let mut f = Fixture::new();
        f.change(|r| {
            r.output_paths.insert("review".into(), value.into());
        });
        let p = f.packet.clone();
        assert!(film_render::bind(&p, &mut f).is_err(), "{value}");
    }
    let mut f = Fixture::new();
    f.change(|r| {
        r.output_paths
            .insert("execution-result".into(), "delivery/result.json".into());
    });
    let p = f.packet.clone();
    assert!(film_render::bind(&p, &mut f).is_err());
    f = Fixture::new();
    f.packet.task.outputs.get_mut("review").unwrap().media_type = "image/png".into();
    f.repin();
    let p = f.packet.clone();
    assert!(film_render::bind(&p, &mut f).is_err());
}
#[test]
fn stale_metadata_open_input_sets_and_missing_programs_refuse() {
    let mut f = Fixture::new();
    f.raw.insert("film/job".into(), b"[]".to_vec());
    let p = f.packet.clone();
    assert_eq!(
        film_render::bind(&p, &mut f).unwrap_err().kind,
        ErrorKind::Integrity
    );
    f = Fixture::new();
    f.packet
        .task
        .inputs
        .push(artifact("extra", b"unbound", "text/plain"));
    f.repin();
    let p = f.packet.clone();
    assert!(film_render::bind(&p, &mut f).is_err());
    f = Fixture::new();
    f.packet
        .task
        .tool
        .as_mut()
        .unwrap()
        .auxiliary_executables
        .remove("ffprobe");
    f.repin();
    let p = f.packet.clone();
    assert!(film_render::bind(&p, &mut f).is_err());
}
#[test]
fn candidate_correspondence_rejects_extra_files_and_cross_task_bound_reuse() {
    let mut f = Fixture::new();
    let p = f.packet.clone();
    let bound = film_render::bind(&p, &mut f).unwrap();
    let mut files: BTreeMap<_, _> = bound
        .request()
        .output_paths
        .values()
        .map(|path| (path.clone(), b"candidate".to_vec()))
        .collect();
    files.insert("extra.png".into(), b"unexpected".to_vec());
    assert!(film_render::capture_outputs(&p, &bound, &files).is_err());
    files.remove("extra.png");
    f.change(|r| r.limits.frames = 96);
    assert_eq!(
        film_render::capture_outputs(&f.packet, &bound, &files)
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
}
