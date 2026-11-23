//! Original declared synthetic exercise; no transcription or human correction.
#![allow(dead_code)] // Shared by separate integration-test crates.
use reel_music::{
    analysis::AnalysisManifest,
    hash::{canonical_sha256, sha256_bytes},
    source::SourceManifest,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub struct ScoreFixture {
    pub temp: tempfile::TempDir,
    pub files: BTreeMap<PathBuf, Vec<u8>>,
}
impl ScoreFixture {
    pub fn new() -> Self {
        Self::with_clock(480, 500000)
    }
    pub fn with_clock(ppq: u64, tempo: u64) -> Self {
        let temp = tempfile::tempdir().unwrap();
        Self::create(ppq, tempo, temp)
    }
    pub fn in_current_directory() -> Self {
        Self::create(
            480,
            500000,
            tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap(),
        )
    }
    fn create(ppq: u64, tempo: u64, temp: tempfile::TempDir) -> Self {
        let ticks = ppq * 9 / 2;
        let frames = (ticks * tempo * 48000 + ppq * 500000) / (ppq * 1000000);
        let pcm: Vec<_> = (0..frames)
            .map(|i| if i % 184 < 92 { 160u8 } else { 96u8 })
            .collect();
        let authority = |hash: &str| json!({"namespace":"public-engineering-fixture","artifact_id":"declared-exercise","content_sha256":hash,"status":"fixture-only","required_roles":["rights-provenance-steward"],"decision_refs":[]});
        let timebase = json!({"pulses_per_quarter":ppq,"rounding":"half-away-from-zero"});
        let source = json!({"schema":"reel.music-source.v0.1","source_id":"exercise-source",
            "media":{"path":"source.raw","sha256":sha256_bytes(&pcm),"decoded_pcm_sha256":sha256_bytes(&pcm),"format":"raw-pcm-u8",
            "timebase":{"sample_rate_hz":48000,"channels":1,"samples_per_channel":frames}},
            "musical_timebase":timebase,"authority":authority(&sha256_bytes(&pcm)),
            "egress":{"private":true,"network_policy":"denied","third_party_upload":false}});
        let source_raw = serde_json::to_vec_pretty(&source).unwrap();
        let source_typed: SourceManifest = serde_json::from_value(source).unwrap();
        let source_contract = canonical_sha256(&source_typed).unwrap();
        let observation = |id: &str, value: Value| {
            json!({"id":id,"analyzer_id":"fixture-declarations","source":{"start":0,"end":frames},
            "confidence_millionths":0,"uncertainty":"Declared synthetic exercise; no acoustic inference or listening approval.","value":value})
        };
        let analysis = json!({"schema":"reel.music-analysis.v0.1","analysis_id":"exercise-declarations",
            "source":{"manifest":"source.json","manifest_sha256":sha256_bytes(&source_raw),"contract_sha256":source_contract,"decoded_pcm_sha256":sha256_bytes(&pcm)},
            "imports":[],"analyzers":[{"id":"fixture-declarations","adapter":"declared-synthetic-fixture","version":"1","model_revision":"none",
                "parameters_sha256":"1".repeat(64),"license":"original-test-data","network_policy":"denied"}],"stems":[],
            "observations":[observation("tempo",json!({"kind":"tempo","milli_bpm":120000})),
                observation("meter",json!({"kind":"meter","numerator":4,"denominator":4})),
                observation("form",json!({"kind":"form","label":"Pickup and A"})),
                observation("pitch",json!({"kind":"pitch","midi_note":60,"cents":0}))],
            "limitations":["Fixture declarations are not recovered musical truth."],
            "review":{"status":"not-reviewed","required_roles":["music-reconstruction-engineer","sound-designer","rights-provenance-steward"],"decision_refs":[]}});
        let analysis_raw = serde_json::to_vec_pretty(&analysis).unwrap();
        let analysis_typed: AnalysisManifest = serde_json::from_value(analysis).unwrap();
        let provenance = |id: &str| {
            json!({"state":"inferred","evidence_refs":[{"analysis_id":"exercise-declarations","observation_id":id}],
            "rationale":"Explicit synthetic fixture hypothesis; not a human correction or musical approval.","correction_ref":null})
        };
        let pickup = ppq / 2;
        let notes= [("pickup-note",0,pickup),("A-one",pickup,ppq),("A-two",pickup+ppq,ppq),("A-three",pickup+ppq*2,ppq*2)]
            .into_iter().map(|(id,start,duration)|json!({"id":id,"voice":1,"start_tick":start,"duration_ticks":duration,"midi_note":60,"velocity":64,"provenance":provenance("pitch")})).collect::<Vec<_>>();
        let model = json!({"schema":"reel.music-model.v0.1","model_id":"pickup-repeat-exercise",
            "source":{"manifest":"source.json","manifest_sha256":sha256_bytes(&source_raw),"contract_sha256":source_contract,"decoded_pcm_sha256":sha256_bytes(&pcm)},
            "analyses":[{"manifest":"analysis.json","manifest_sha256":sha256_bytes(&analysis_raw),"contract_sha256":canonical_sha256(&analysis_typed).unwrap(),"analysis_id":"exercise-declarations"}],
            "authority":authority(&sha256_bytes(&analysis_raw)),"musical_timebase":timebase,"duration_ticks":ticks,
            "tempo_map":[{"tick":0,"microseconds_per_quarter":tempo,"provenance":provenance("tempo")}],
            "meter_map":[{"tick":0,"numerator":4,"denominator":4,"provenance":provenance("meter")}],
            "form":[{"id":"pickup","label":"Pickup","range":{"start":0,"end":pickup},"provenance":provenance("form")},
                {"id":"A","label":"A","range":{"start":pickup,"end":ticks},"provenance":provenance("form")}],
            "parts":[{"id":"melody","role":"melody","name":"Exercise","notes":notes}],
            "instrumental_score":{"title":"Pickup and A","parts":[{"id":"exercise","name":"Exercise","staff_part_ids":["melody"],"clef":"treble"}],"pickup_ticks":pickup,"key_fifths":0},
            "unknowns":["Musical usefulness and acoustic fidelity are not reviewed."],
            "review":{"status":"not-reviewed","required_roles":["music-reconstruction-engineer","score-arrangement-director","sound-designer","editor","rights-provenance-steward"],"decision_refs":[]}});
        let files = BTreeMap::from([
            (PathBuf::from("source.raw"), pcm),
            (PathBuf::from("source.json"), source_raw),
            (PathBuf::from("analysis.json"), analysis_raw),
            (
                PathBuf::from("model.json"),
                serde_json::to_vec_pretty(&model).unwrap(),
            ),
        ]);
        for (name, raw) in &files {
            fs::write(temp.path().join(name), raw).unwrap();
        }
        Self { temp, files }
    }
    pub fn model(&self) -> PathBuf {
        self.temp.path().join("model.json")
    }
    pub fn bytes(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("fixture provider refused unknown name"))
    }
}
