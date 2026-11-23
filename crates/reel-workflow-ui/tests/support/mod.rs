#![allow(dead_code)]
use reel_workflow::{
    Artifact, Error, ErrorKind, Result,
    handoff::{Journal, Lifecycle, ProtectedJob, Ticket},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
pub fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    fs::write(root.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
pub fn asset(path: &Path, id: &str, mime: &str) -> Artifact {
    let raw = fs::read(path).unwrap();
    Artifact {
        logical_id: id.into(),
        sha256: reel_workflow::digest(&raw),
        bytes: raw.len() as u64,
        media_type: mime.into(),
    }
}
pub fn picture(path: &Path, variant: u8) {
    let (w, h) = (640usize, 360usize);
    let mut pixels = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let mut c = if y < 195 {
                [182, 199, 193]
            } else {
                [54, 102, 116]
            };
            if variant == 2 {
                c = if y < 195 {
                    [223, 174, 140]
                } else {
                    [70, 89, 112]
                };
            }
            if y < 195 && ((x as i32 - 470).pow(2) + (y as i32 - 75).pow(2)) < 900 {
                c = [245, 224, 166];
            }
            let hill = 165 + ((x as f64 / 90.0 + f64::from(variant)).sin() * 23.0) as i32;
            if y as i32 > hill && y < 205 {
                c = if variant == 2 {
                    [83, 87, 86]
                } else {
                    [71, 108, 104]
                };
            }
            if (120..190).contains(&x) && (135..215).contains(&y) {
                c = [219, 194, 152];
            }
            if (130..144).contains(&x) && (155..180).contains(&y) {
                c = [72, 96, 98];
            }
            if y > 125
                && y < 142
                && x > 110
                && x < 200
                && (x as i32 - 155).abs() < ((y - 125) * 4) as i32
            {
                c = [128, 91, 76];
            }
            if y > 220
                && y < 235
                && x > 280 + usize::from(variant) * 40
                && x < 390 + usize::from(variant) * 40
            {
                c = [33, 53, 64];
            }
            let mast = 335 + usize::from(variant) * 40;
            if x.abs_diff(mast) < 2 && (145..225).contains(&y) {
                c = [229, 215, 182];
            }
            if (150..215).contains(&y) && x > mast + 4 && x < mast + 4 + (y - 150) {
                c = [238, 222, 190];
            }
            if y > 245 && y % 21 < 2 && (x + y) % 130 < 55 {
                c = [108, 148, 151];
            }
            let p = (y * w + x) * 3;
            pixels[p..p + 3].copy_from_slice(&c);
        }
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    let mut encoder = png::Encoder::new(file, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
}
pub fn tone(path: &Path, seconds: u32, hz: f64) {
    let rate = 24000u32;
    let count = rate * seconds;
    let size = count * 2;
    let mut raw = Vec::new();
    raw.extend(b"RIFF");
    raw.extend((36 + size).to_le_bytes());
    raw.extend(b"WAVEfmt ");
    raw.extend(16u32.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(rate.to_le_bytes());
    raw.extend((rate * 2).to_le_bytes());
    raw.extend(2u16.to_le_bytes());
    raw.extend(16u16.to_le_bytes());
    raw.extend(b"data");
    raw.extend(size.to_le_bytes());
    for n in 0..count {
        let fade = (f64::from(n) / 2400.0).min(1.0) * (f64::from(count - n) / 2400.0).min(1.0);
        let value = ((f64::from(n) * hz * std::f64::consts::TAU / f64::from(rate)).sin()
            * 1800.0
            * fade) as i16;
        raw.extend(value.to_le_bytes());
    }
    fs::write(path, raw).unwrap();
}
pub struct Synthetic {
    pub pending: bool,
}
impl ProtectedJob for Synthetic {
    fn register(&mut self, _: &Ticket, _: &reel_workflow::ReturnedResult) -> Result<Artifact> {
        if self.pending {
            return Err(Error {
                kind: ErrorKind::NeedsRecovery,
                code: "synthetic-pending".into(),
            });
        }
        Ok(Artifact {
            logical_id: "synthetic/ack".into(),
            sha256: reel_workflow::digest(b"synthetic acknowledgement, not native proof"),
            bytes: 44,
            media_type: "application/json".into(),
        })
    }
    fn reconcile(&mut self, t: &Ticket, r: &reel_workflow::ReturnedResult) -> Result<Artifact> {
        self.register(t, r)
    }
    fn verify_retained(
        &mut self,
        _: &Ticket,
        _: &reel_workflow::ReturnedResult,
        _: &Artifact,
    ) -> Result<()> {
        Ok(())
    }
}
impl Lifecycle for Synthetic {
    type Guard<'a> = Self;
    fn reopen(&mut self, _: &Ticket) -> Result<Self> {
        Ok(Self {
            pending: self.pending,
        })
    }
}
pub fn create(root: &Path) -> Value {
    fs::create_dir(root).unwrap();
    picture(&root.join("cel-1.png"), 0);
    picture(&root.join("cel-2.png"), 1);
    picture(&root.join("prior.png"), 3);
    tone(&root.join("es.wav"), 6, 330.0);
    tone(&root.join("en.wav"), 8, 440.0);
    let plan:reel_workflow::Plan=serde_json::from_value(json!({"schema":reel_workflow::PLAN_SCHEMA,"project":"studio-sample","source_commit":"a".repeat(40),"tasks":[{"id":"redraw-c002","operation":"image-candidate","language":null,"prompt":"Synthetic fixture candidate.","inputs":[],"depends_on":[],"outputs":{"picture.png":{"media_type":"image/png","max_bytes":2097152,"required":true}}}]})).unwrap();
    let packet = reel_workflow::make_packet(&plan, "redraw-c002", BTreeMap::new()).unwrap();
    let descriptor = Artifact {
        logical_id: "synthetic/binding".into(),
        sha256: reel_workflow::digest(b"synthetic"),
        bytes: 9,
        media_type: "application/json".into(),
    };
    let ticket = Ticket {
        packet,
        attempt_id: "attempt-1".into(),
        native_binding: descriptor.clone(),
        lifecycle_tool: descriptor,
    };
    Journal::create(
        &root.join("journal"),
        ticket.clone(),
        &mut Synthetic { pending: false },
    )
    .unwrap();
    let a = asset(&root.join("cel-1.png"), "picture/c001/v1", "image/png");
    let b = asset(&root.join("cel-2.png"), "picture/c002/v1", "image/png");
    let prior = asset(&root.join("prior.png"), "picture/c001/v0", "image/png");
    let es = asset(&root.join("es.wav"), "recording/cue/es", "audio/wav");
    let en = asset(&root.join("en.wav"), "recording/cue/en", "audio/wav");
    let spans = |start_es: u64, end_es: u64, start_en: u64, end_en: u64| json!([{"language":"es","cue_id":"cue-1","start_sample":start_es,"end_sample":end_es},{"language":"en","cue_id":"cue-1","start_sample":start_en,"end_sample":end_en}]);
    let version = |id: &str, label: &str, asset: &Artifact, designation: &str, preview: &str| json!({"id":id,"label":label,"asset":asset,"designation":designation,"language":null,"preview":preview,"approval_reference":null});
    let board = json!({"schema":"reel.workflow-board.v1","project":"studio-sample","work_id":"harbor-study","title":"A little harbor","format":"film","source_commit":"a".repeat(40),"synthetic":true,"languages":[{"id":"es","label":"Español"},{"id":"en","label":"English"}],"scenes":[{"id":"scene-1","title":"01 · Across the water"}],"units":[
        {"id":"event-1","story_id":"story-01","display_id":"C001","semantic_id":"arrival","scene_id":"scene-1","title":"The harbor wakes","versions":[version("v0","v0 · previous",&prior,"historical","prior"),version("v1","v1 · selected",&a,"selected","cel1")],"spans":spans(0,72000,0,96000)},
        {"id":"event-2","story_id":"story-01","display_id":"C002","semantic_id":"crossing","scene_id":"scene-1","title":"A sail on the horizon","versions":[version("v1","v1 · selected",&b,"selected","cel2")],"spans":spans(72000,144000,96000,192000),"task":"redraw-c002"},
        {"id":"event-3","story_id":"story-01","display_id":"C003","semantic_id":"departure","scene_id":"scene-1","title":"The next moment","versions":[],"spans":[]}
    ],"cues":[
        {"id":"cue-1","semantic_id":"opening","language":"es","title":"Cue 01 · Prueba de sonido","text":"Primera señal. Segunda señal. Esta pista es un tono de prueba, no una interpretación de voz.","selected_take":es,"preview":"es","sample_rate":24000,"samples":144000,"markers":[{"semantic_id":"arrival","word":"Primera señal","sample":0},{"semantic_id":"crossing","word":"Segunda señal","sample":72000}]},
        {"id":"cue-1","semantic_id":"opening","language":"en","title":"Cue 01 · Sound check","text":"First signal. Second signal. This track is a test tone, not a voice performance.","selected_take":en,"preview":"en","sample_rate":24000,"samples":192000,"markers":[{"semantic_id":"arrival","word":"First signal","sample":0},{"semantic_id":"crossing","word":"Second signal","sample":96000}]}
    ],"media":{"cel1":{"artifact":a,"path":root.join("cel-1.png"),"subject":null},"cel2":{"artifact":b,"path":root.join("cel-2.png"),"subject":null},"prior":{"artifact":prior,"path":root.join("prior.png"),"subject":null},"es":{"artifact":es,"path":root.join("es.wav"),"subject":null},"en":{"artifact":en,"path":root.join("en.wav"),"subject":null}},"tasks":{"redraw-c002":{"journal":root.join("journal"),"ticket_sha256":reel_workflow::canonical_digest(&ticket).unwrap(),"task_key":ticket.packet.task_key,"attempt_id":"attempt-1","outputs":{"picture.png":{"unit_id":"event-2","path":root.join("candidate.png"),"label":"v2 · returned"}}}}});
    let typed: reel_workflow_ui::Board = serde_json::from_value(board.clone()).unwrap();
    typed.check().unwrap();
    save(root, "board.json", &board);
    let mut comic = board.clone();
    comic["format"] = json!("comic");
    comic["title"] = json!("Harbor · Page studies");
    save(root, "comic-board.json", &comic);
    board
}
pub fn advance(root: &Path, retain: bool) {
    let mut journal = Journal::open(&root.join("journal")).unwrap();
    if retain {
        journal.resume(&mut Synthetic { pending: false }).unwrap();
        return;
    }
    picture(&root.join("candidate.png"), 2);
    let candidate = asset(
        &root.join("candidate.png"),
        "picture/c002/candidate",
        "image/png",
    );
    let result = reel_workflow::ReturnedResult {
        schema: reel_workflow::RESULT_SCHEMA.into(),
        task_id: journal.ticket().packet.task.id.clone(),
        task_key: journal.ticket().packet.task_key.clone(),
        attempt_id: journal.ticket().attempt_id.clone(),
        artifacts: BTreeMap::from([("picture.png".into(), candidate)]),
        provenance: reel_workflow::Provenance {
            mode: reel_workflow::Mode::Direct,
            provider: Some("synthetic".into()),
            model: Some("fixture-v1".into()),
            executable: None,
        },
        worker_receipt: None,
    };
    assert_eq!(
        journal
            .import(result, &mut Synthetic { pending: true })
            .unwrap_err()
            .code,
        "synthetic-pending"
    );
}
