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
