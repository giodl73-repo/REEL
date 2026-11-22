use reel::scene_delivery::FileRef;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};
pub fn reference(path: &Path, name: &str) -> FileRef {
    let raw = fs::read(path).unwrap();
    FileRef {
        path: name.into(),
        sha256: reel_workflow::digest(&raw),
        bytes: raw.len() as u64,
    }
}
pub fn fixture(root: &Path) -> BTreeMap<String, FileRef> {
    fs::write(
        root.join("red.ppm"),
        [b"P6\n2 2\n255\n".as_slice(), &[255, 0, 0].repeat(4)].concat(),
    )
    .unwrap();
    let samples = 4800u32;
    let size = samples * 6;
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + size).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(288000u32.to_le_bytes());
    wav.extend(6u16.to_le_bytes());
    wav.extend(24u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(size.to_le_bytes());
    wav.resize(44 + size as usize, 0);
    fs::write(root.join("a.wav"), wav).unwrap();
    let write = |name: &str, value: serde_json::Value| {
        fs::write(root.join(name), serde_json::to_vec(&value).unwrap()).unwrap()
    };
    write(
        "production.json",
        json!({"manifest_version":"reel.manifest.v0.2","profile":"animatic","timing_status":"conformed","work":"synthetic","title":"Synthetic","scenes":[{"id":"scene"}],"shots":[{"id":"shot","scene_id":"scene"}],"narration_cues":[{"id":"a","speaker_id":"narrator"}],"audio_events":[{"id":"a","role":"narration","source":"a.wav","start_seconds":0}]}),
    );
    write(
        "contract.json",
        json!({"schema":"reel.cue-relative-assembly.v0.1","id":"scene","production_manifest":"production.json","sample_rate":48000,"frame_rate":{"numerator":24,"denominator":1},"cues":[{"cue_id":"a","duration_samples":samples}],"attachments":[
            {"id":"p-a","target":{"kind":"cel","shot_id":"shot","cel_id":"red"},"start":{"kind":"cue-start","cue_id":"a"},"end":{"kind":"cue-end","cue_id":"a"}},
            {"id":"d-a","target":{"kind":"audio","audio_event_id":"a"},"start":{"kind":"cue-start","cue_id":"a"},"end":{"kind":"cue-end","cue_id":"a"}}
        ]}),
    );
    write(
        "job.json",
        json!({"schema":"reel.scene-delivery.v0.1","id":"scene","contract":reference(&root.join("contract.json"),"contract.json"),"production_manifest_sha256":reference(&root.join("production.json"),"production.json").sha256,"width":64,"height":64,"max_composition_samples":480000,"pictures":[{"attachment_id":"p-a","source":reference(&root.join("red.ppm"),"red.ppm"),"kind":"still","attention":"Synthetic"}],"audio":[{"attachment_id":"d-a","source":reference(&root.join("a.wav"),"a.wav"),"bus":"D","cue_id":"a"}],"buses":{"D":{"state":"present","reason":"Synthetic"},"M":{"state":"intentional-silence","reason":"Synthetic"},"E":{"state":"intentional-silence","reason":"Synthetic"}}}),
    );
    [
        "production.json",
        "contract.json",
        "job.json",
        "red.ppm",
        "a.wav",
    ]
    .into_iter()
    .map(|name| {
        (
            name.into(),
            reference(&root.join(name), root.join(name).to_str().unwrap()),
        )
    })
    .collect()
}
