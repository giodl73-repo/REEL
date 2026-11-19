use reel_workflow::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub struct Fixture {
    pub packet: Packet,
    pub values: BTreeMap<String, (Artifact, Vec<u8>)>,
}
fn put(
    values: &mut BTreeMap<String, (Artifact, Vec<u8>)>,
    path: &str,
    raw: Vec<u8>,
    media: &str,
) -> Artifact {
    let a = Artifact {
        logical_id: format!("film/{path}"),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: media.into(),
    };
    values.insert(path.into(), (a.clone(), raw));
    a
}
fn json_put(
    values: &mut BTreeMap<String, (Artifact, Vec<u8>)>,
    path: &str,
    value: Value,
) -> Artifact {
    put(
        values,
        path,
        serde_json::to_vec(&value).unwrap(),
        "application/json",
    )
}
fn binding(a: &Artifact) -> Value {
    json!({"logical_id":a.logical_id,"sha256":a.sha256,"bytes":a.bytes,"cache_uri":format!("cache://sha256/{}",a.sha256),"selection_state":"selected-private-production"})
}
fn slot(id: &str, lane: &str, a: &Artifact) -> Value {
    json!({"slot_id":id,"beat_id":"beat","lane":lane,"disposition":"selected","selected_revision_id":format!("{id}-r1"),"revisions":[{"revision_id":format!("{id}-r1"),"supersedes":null,"asset":{"logical_id":a.logical_id,"sha256":a.sha256,"cache_uri":format!("cache://sha256/{}",a.sha256)}}]})
}
pub fn wave(frames: u32) -> Vec<u8> {
    let mut raw = b"RIFF".to_vec();
    raw.extend((frames * 2 + 36).to_le_bytes());
    raw.extend(b"WAVEfmt ");
    raw.extend(16u32.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(8000u32.to_le_bytes());
    raw.extend(16000u32.to_le_bytes());
    raw.extend(2u16.to_le_bytes());
    raw.extend(16u16.to_le_bytes());
    raw.extend(b"data");
    raw.extend((frames * 2).to_le_bytes());
    raw.resize(44 + frames as usize * 2, 0);
    raw
}
impl Fixture {
    pub fn new(language: &str) -> Self {
        let mut values = BTreeMap::new();
        let (text, cut, frames) = if language == "es" {
            ("Hola mira", 2400u64, 8000u32)
        } else {
            ("Hello look", 4500, 12000)
        };
        let text_a = put(
            &mut values,
            "text.txt",
            text.as_bytes().to_vec(),
            "text/plain",
        );
        let take = put(&mut values, "take.wav", wave(frames), "audio/wav");
        let p1 = put(
            &mut values,
            "one.svg",
            br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.to_vec(),
            "image/svg+xml",
        );
        let p2 = put(
            &mut values,
            "two.svg",
            br#"<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>"#.to_vec(),
            "image/svg+xml",
        );
        json_put(
            &mut values,
            "words.json",
            json!({"schema":"reel.scene-word-timing-evidence.v1","language":language,"cue_id":"cue-1","selected_take_sha256":take.sha256,"sample_rate":8000,"cue_end_sample":frames,"words":[{"word":text.split_whitespace().next().unwrap(),"start_sample":100,"end_sample":1000},{"word":text.split_whitespace().nth(1).unwrap(),"start_sample":cut,"end_sample":cut+1000}]}),
        );
        let mut graph:reel_assembly::Graph=serde_json::from_value(json!({"schema":"reel.semantic-assembly.v1","lock":{"logical_id":"lock-1","sha256":"a".repeat(64)},"slots":[slot("narration","narration",&take),slot("picture-1","picture",&p1),slot("picture-2","picture",&p2)],"events":[],"nodes":[{"id":"scene-1","inputs":[],"slots":["narration","picture-1","picture-2"],"events":[]}],"presentation_targets":[]})).unwrap();
        graph.lock = reel_assembly::graph_lock(&graph, "lock-1").unwrap();
        let pointer = reel_assembly::advance_pointer("selected", &graph).unwrap();
        json_put(
            &mut values,
            "graph.json",
            serde_json::to_value(graph).unwrap(),
        );
        json_put(
            &mut values,
            "pointer.json",
            serde_json::to_value(pointer).unwrap(),
        );
        json_put(
            &mut values,
            "episode.json",
            json!({"schema":"reel.episode-authoring.v1","episode_id":"episode-1","season_id":"season-1","authoring_state":"ready-for-private-build","master_template_id":"master","scene_policy_id":"policy","presentation":[],"score_palette":[],"scene_ids":["scene-1"]}),
        );
        json_put(
            &mut values,
            "scene-scope.json",
            json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"scene-1","assets":{"take":binding(&take),"one":binding(&p1),"two":binding(&p2)}}),
        );
        json_put(
            &mut values,
            "episode-scope.json",
            json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"episode-1","assets":{}}),
        );
        json_put(
            &mut values,
            "season-scope.json",
            json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"season-1","assets":{}}),
        );
        let scene = json_put(
            &mut values,
            "scene.json",
            json!({"schema":"reel.scene-authoring.v2","scene_id":"scene-1","episode_id":"episode-1","authoring_state":"ready-for-private-build","source_authority_id":"fixture","canonical_cue_ids":["cue-1"],
            "shared_events":[{"semantic_id":"greeting","canonical_cue_id":"cue-1","picture_binding":"one","score":{"disposition":"silence"}},{"semantic_id":"look","canonical_cue_id":"cue-1","picture_binding":"two","score":{"disposition":"silence"}}],
            "languages":{language:{"cues":[{"cue_id":"cue-1","source_id":"cue-1","exact_text_sha256":text_a.sha256,"narration_slot_id":"narration","take_binding":"take"}]}},
            "language_event_bindings":{language:[{"semantic_id":"greeting","event_id":"greeting","cue_id":"cue-1","semantic_trigger_id":"greeting","picture_slot_id":"picture-1","spoken_trigger_phrase":text.split_whitespace().next().unwrap()},{"semantic_id":"look","event_id":"look","cue_id":"cue-1","semantic_trigger_id":"look","picture_slot_id":"picture-2","spoken_trigger_phrase":text.split_whitespace().nth(1).unwrap()}]}}),
        );
        let request = json_put(
            &mut values,
            "request.json",
            json!({"schema":"reel.workflow-film-scene-control.v1","graph_path":"graph.json","pointer_path":"pointer.json","episode_path":"episode.json","scope_paths":["scene-scope.json","episode-scope.json","season-scope.json"],"cues":{"cue-1":{"text_path":"text.txt","take_path":"take.wav","words_path":"words.json"}},"next_lock_logical_id":"lock-2"}),
        );
        let task = Task {
            id: "film-control".into(),
            native_inputs: None,
            upstream_inputs: None,
            operation: Operation::DeterministicTool,
            tool: Some(ToolContract {
                auxiliary_executables: BTreeMap::new(),
                schema: "reel.workflow-tool.v1".into(),
                capability: ToolCapability::FilmSceneControl,
                executable: Artifact {
                    logical_id: "tools/reel".into(),
                    sha256: "d".repeat(64),
                    bytes: 100,
                    media_type: "application/octet-stream".into(),
                },
                publication: scene,
                edition_id: language.into(),
                request,
                source_bindings: values
                    .iter()
                    .filter(|(p, _)| p.as_str() != "request.json" && p.as_str() != "scene.json")
                    .map(|(p, (a, _))| (p.clone(), a.logical_id.clone()))
                    .collect(),
                part: None,
            }),
            language: Some(language.into()),
            prompt: String::new(),
            inputs: values.values().map(|(a, _)| a.clone()).collect(),
            depends_on: vec![],
            outputs: film::OUTPUTS
                .into_iter()
                .map(|n| {
                    (
                        n.into(),
                        OutputContract {
                            media_type: "application/json".into(),
                            max_bytes: MAX_CONTROL_BYTES as u64,
                            required: true,
                        },
                    )
                })
                .collect(),
        };
        let packet = make_packet(
            &Plan {
                schema: PLAN_SCHEMA.into(),
                project: "synthetic-film".into(),
                source_commit: "a".repeat(40),
                tasks: vec![task],
            },
            "film-control",
            BTreeMap::new(),
        )
        .unwrap();
        Self { packet, values }
    }
    pub fn change_json(&mut self, path: &str, edit: impl FnOnce(&mut Value)) {
        let (old, raw) = self.values.get(path).unwrap();
        let mut value: Value = serde_json::from_slice(raw).unwrap();
        edit(&mut value);
        let old = old.clone();
        let new = json_put(&mut self.values, path, value);
        for a in &mut self.packet.task.inputs {
            if a.logical_id == old.logical_id {
                *a = new.clone();
            }
        }
        let t = self.packet.task.tool.as_mut().unwrap();
        if t.publication.logical_id == old.logical_id {
            t.publication = new.clone();
        }
        if t.request.logical_id == old.logical_id {
            t.request = new;
        }
        self.packet = make_packet(
            &Plan {
                schema: PLAN_SCHEMA.into(),
                project: self.packet.project.clone(),
                source_commit: self.packet.source_commit.clone(),
                tasks: vec![self.packet.task.clone()],
            },
            "film-control",
            BTreeMap::new(),
        )
        .unwrap();
    }
}
impl publication::InputReader for Fixture {
    fn read(&mut self, a: &Artifact, _: u64) -> Result<Vec<u8>> {
        self.values
            .values()
            .find(|(s, _)| s.logical_id == a.logical_id)
            .map(|(_, r)| r.clone())
            .ok_or(Error {
                kind: ErrorKind::Missing,
                code: "fixture-input".into(),
            })
    }
}
