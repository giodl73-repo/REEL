use reel_workflow::{Artifact, Result, digest, film_review::*};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Default)]
pub struct Memory {
    pub files: BTreeMap<String, Vec<u8>>,
    pub verified: Vec<String>,
}
impl Reader for Memory {
    fn read(&mut self, a: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        assert!(a.bytes <= maximum);
        Ok(self.files[&a.logical_id].clone())
    }
    fn verify(&mut self, a: &Artifact, maximum: u64) -> Result<()> {
        assert!(a.bytes <= maximum);
        let raw = &self.files[&a.logical_id];
        assert_eq!(raw.len() as u64, a.bytes);
        assert_eq!(digest(raw), a.sha256);
        self.verified.push(a.logical_id.clone());
        Ok(())
    }
}
pub fn artifact(memory: &mut Memory, id: &str, raw: Vec<u8>, mime: &str) -> Artifact {
    let a = Artifact {
        logical_id: id.into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: mime.into(),
    };
    memory.files.insert(id.into(), raw);
    a
}
pub fn control(memory: &mut Memory, id: &str, value: Value) -> Artifact {
    artifact(
        memory,
        id,
        serde_json::to_vec(&value).unwrap(),
        "application/json",
    )
}
pub fn fixture() -> (Report, Memory) {
    let mut memory = Memory::default();
    let file = |path: &str, sha: &str| json!({"path":path,"sha256":sha.repeat(64),"bytes":100});
    let movie = artifact(
        &mut memory,
        "review/movie",
        b"Synthetic byte fixture; not a playable film.".to_vec(),
        "video/mp4",
    );
    let map = control(
        &mut memory,
        "review/map",
        json!({"schema":"reel.film-cel-review-map.v1","request_sha256":"a".repeat(64),"project":"synthetic-film","source_commit":"b".repeat(40),"language":"es", "source_film":file("delivery/review.mp4","c"),"delivery_receipt":file("delivery/receipt.json","d"),"job":file("job.json","e"),"frame_count":48,"fps_numerator":24,"fps_denominator":1,"width":320,"height":180,"targets":[
            {"attachment_id":"p-a","story_id":"story-1","cel_id":"C100","semantic_id":"arrival","selected_picture":file("picture.png","7"),"start_frame":0,"end_frame":24},
            {"attachment_id":"p-b","story_id":"story-1","cel_id":"C101","semantic_id":"departure","selected_picture":file("other.png","8"),"start_frame":24,"end_frame":48}
        ]}),
    );
    let receipt = control(
        &mut memory,
        "review/receipt",
        json!({"schema":"reel.film-cel-review-receipt.v1","request_sha256":"a".repeat(64),"source_clocks_sha256":"f".repeat(64),"output_clocks_sha256":"f".repeat(64),"frame_count":48,"audio_packet_count":95,"outputs":{
        "review.mp4":{"path":"review.mp4","sha256":movie.sha256,"bytes":movie.bytes},"review-map.json":{"path":"review-map.json","sha256":map.sha256,"bytes":map.bytes},
        "font.ttf":file("font.ttf","1"),"filters.txt":file("filters.txt","2"),"label-0.txt":file("label-0.txt","3"),"label-1.txt":file("label-1.txt","4")},"publication":"not-authorized","visual_reviewed":false,"approvals_verified":false,"native_retention_verified":false}),
    );
    let report = Report {
        schema: REPORT_SCHEMA.into(),
        review: movie,
        map,
        receipt,
        findings: vec![
            Finding {
                id: "finding-1".into(),
                target_id: "p-a".into(),
                frame: 23,
                category: Category::Picture,
                method: Method::ModelReport,
                observation: "Revisar el detalle de la orilla.\nConservar el texto español.".into(),
                evidence: vec![],
                board_reference: None,
            },
            Finding {
                id: "finding-2".into(),
                target_id: "p-b".into(),
                frame: 24,
                category: Category::Timing,
                method: Method::HumanReport,
                observation: "Check the transition on this language's clock.".into(),
                evidence: vec![],
                board_reference: None,
            },
        ],
    };
    (report, memory)
}
