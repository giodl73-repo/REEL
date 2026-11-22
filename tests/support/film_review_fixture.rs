#![allow(dead_code)]
use reel::scene_delivery::{self, FileRef};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    fs::write(root.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
pub fn reference(root: &Path, name: &str) -> Value {
    let path = root.join(name);
    let hash: String = Sha256::digest(fs::read(&path).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"path":name,"sha256":hash,"bytes":fs::metadata(path).unwrap().len()})
}
pub fn wav(path: &Path, samples: u32) {
    let size = samples * 6;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + size).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend(288000u32.to_le_bytes());
    b.extend(6u16.to_le_bytes());
    b.extend(24u16.to_le_bytes());
    b.extend(b"data");
    b.extend(size.to_le_bytes());
    for _ in 0..samples {
        b.extend([0; 6]);
    }
    fs::write(path, b).unwrap();
}
pub fn fixture(root: &Path) -> Value {
    for (name, color) in [("red", [180u8, 30, 30]), ("blue", [20, 40, 170])] {
        fs::write(
            root.join(format!("{name}.ppm")),
            [b"P6\n2 2\n255\n".as_slice(), &color.repeat(4)].concat(),
        )
        .unwrap();
    }
    wav(&root.join("a.wav"), 48001);
    wav(&root.join("b.wav"), 47999);
    save(
        root,
        "production.json",
        &json!({"manifest_version":"reel.manifest.v0.2","profile":"animatic","timing_status":"conformed","work":"synthetic","title":"Synthetic review","scenes":[{"id":"scene"}],"shots":[{"id":"shot","scene_id":"scene"}],"narration_cues":[{"id":"a","speaker_id":"narrator"},{"id":"b","speaker_id":"narrator"}],"audio_events":[{"id":"a","role":"narration","source":"a.wav","start_seconds":0},{"id":"b","role":"narration","source":"b.wav","start_seconds":0}]}),
    );
    let anchor = |cue: &str, edge: &str| json!({"kind":format!("cue-{edge}"),"cue_id":cue});
    save(
        root,
        "contract.json",
        &json!({"schema":"reel.cue-relative-assembly.v0.1","id":"scene","production_manifest":"production.json","sample_rate":48000,"frame_rate":{"numerator":24,"denominator":1},"cues":[{"cue_id":"a","duration_samples":48001},{"cue_id":"b","duration_samples":47999}],"attachments":[
            {"id":"p-a","target":{"kind":"cel","shot_id":"shot","cel_id":"first"},"start":anchor("a","start"),"end":anchor("a","end")},
            {"id":"p-b","target":{"kind":"cel","shot_id":"shot","cel_id":"second"},"start":anchor("b","start"),"end":anchor("b","end")},
            {"id":"d-a","target":{"kind":"audio","audio_event_id":"a"},"start":anchor("a","start"),"end":anchor("a","end")},
            {"id":"d-b","target":{"kind":"audio","audio_event_id":"b"},"start":anchor("b","start"),"end":anchor("b","end")}
        ]}),
    );
    let job = json!({"schema":"reel.scene-delivery.v0.1","id":"scene","contract":reference(root,"contract.json"),"production_manifest_sha256":reference(root,"production.json")["sha256"],"width":320,"height":180,"max_composition_samples":96000,
        "pictures":[{"attachment_id":"p-a","source":reference(root,"red.ppm"),"kind":"still","attention":"Synthetic first cel","delivery_frame_count":24},{"attachment_id":"p-b","source":reference(root,"blue.ppm"),"kind":"still","attention":"Synthetic second cel","delivery_frame_count":24}],
        "audio":[{"attachment_id":"d-a","source":reference(root,"a.wav"),"bus":"D","cue_id":"a"},{"attachment_id":"d-b","source":reference(root,"b.wav"),"bus":"D","cue_id":"b"}],
        "buses":{"D":{"state":"present","reason":"Synthetic silence"},"M":{"state":"intentional-silence","reason":"No music"},"E":{"state":"intentional-silence","reason":"No effects"}}});
    save(root, "job.json", &job);
    scene_delivery::render(&root.join("job.json"), root, &root.join("source")).unwrap();
    let font = [
        "C:/Windows/Fonts/arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
    .unwrap();
    fs::copy(font, root.join("selected-font.ttf")).unwrap();
    let request = json!({"schema":"reel.film-cel-review.v1","project":"synthetic-film","source_commit":"a".repeat(40),"language":"es","film":reference(root,"source/review.mp4"),"delivery_receipt":reference(root,"source/receipt.json"),"job":reference(root,"job.json"),"font":reference(root,"selected-font.ttf"),"font_size":18,
        "identities":{"p-a":{"story_id":"story-1","cel_id":"C100","semantic_id":"arrival"},"p-b":{"story_id":"story-1","cel_id":"C101","semantic_id":"departure"}},
        "limits":{"timeout_seconds":120,"input_bytes":32*1024*1024,"output_bytes":16*1024*1024,"probe_bytes":4*1024*1024,"frames":100}});
    save(root, "request.json", &request);
    request
}
pub fn manifest(root: &Path) -> FileRef {
    let mut file: FileRef = serde_json::from_value(reference(root, "request.json")).unwrap();
    file.path = root.join("request.json");
    file
}
pub fn frame(path: &Path, frame: u64) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("select=eq(n\\,{frame})"),
            "-frames:v",
            "1",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 320 * 180 * 3);
    output.stdout
}
