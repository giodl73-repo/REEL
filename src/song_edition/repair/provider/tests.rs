use super::*;
use crate::song_edition::{
    repair::tests::inputs,
    source::tests::{Fixture, fixture},
};
use std::cell::Cell;

fn save(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap()
}
fn descriptor(path: &Path) -> Value {
    serde_json::to_value(pin(&fs::read(path).unwrap())).unwrap()
}
fn execution(f: &Fixture) -> PathBuf {
    let root = f.dir.path();
    for (name, bytes) in [
        ("local_song_repaint.py", b"# synthetic provider".as_slice()),
        ("local_song_backend.py", b"# synthetic helper"),
        ("model.bin", b"synthetic model"),
        ("python.exe", b"synthetic installed runtime"),
    ] {
        fs::write(root.join(name), bytes).unwrap()
    }
    let installed = |name: &str| {
        let mut v = descriptor(&root.join(name));
        v["path"] = json!(root.join(name));
        v
    };
    let runtime = json!({"schema":"reel.local-song-runtime.v1","ace_root":root,"ace_commit":"a".repeat(40),
        "python":installed("python.exe"),"generation_files":[installed("model.bin")]});
    save(&root.join("runtime-input.json"), &runtime);
    let input = |name: &str| json!({"path":name,"object":descriptor(&root.join(name))});
    let path = root.join("execution.json");
    save(
        &path,
        &json!({"schema":"reel.original-song-repair-execution.v1","runtime":input("runtime-input.json"),
        "backend":input("local_song_repaint.py"),"backend_helper":input("local_song_backend.py"),
        "caption":"Synthetic private repair fixture","seed":7,"repaint_mode":"conservative","repaint_strength":0.5,
        "permissions":{"private_only":true,"external_upload":false,"identifiable_imitation":false,"public_release":false},
        "timeout_seconds":10,"memory_bytes":536870912u64,"processes":4}),
    );
    path
}
fn fake(inv: &Invocation<'_>, mutation: usize) -> Result<bool> {
    let request = packet::parse(&fs::read(inv.output.join("provider-request.json"))?)?;
    let mut attempts = Vec::new();
    for (index, geometry) in inv.prepared.loaded.geometry.iter().enumerate() {
        let start = geometry["context_start_frame"].as_u64().unwrap();
        let end = geometry["context_end_frame"].as_u64().unwrap();
        let frames = if mutation == 2 {
            end - start + 1000
        } else {
            end - start + 100
        };
        let rate = if mutation == 3 {
            16000
        } else {
            inv.prepared.loaded.audio.rate
        };
        let peak = if mutation == 4 { 1.25 } else { 0.5 };
        let raw = wave::encode(&[peak, -peak].repeat(frames as usize), rate, 2)?;
        write_new(&inv.output.join(format!("raw-patch-{index}.wav")), &raw)?;
        attempts.push(json!({"index":index,"attempt":if mutation==1 {2}else{1},"seed":7+index,
            "context_start_frame":start,"context_end_frame":end,"context_audio":request["patches"][index]["context_audio"],"raw_audio":pin(&raw)}));
    }
    let mut receipt = json!({"schema":"reel.original-song-repair-provider-receipt.v1",
        "request":descriptor(&inv.output.join("provider-request.json")),"runtime":request["runtime"],
        "source_context":request["source_context"],"patches":attempts,"external_upload":false,"human_approval":false,"selected":false});
    if mutation == 5 {
        receipt["runtime"]["sha256"] = json!("0".repeat(64));
    }
    save(&inv.output.join("provider-receipt.json"), &receipt);
    Ok(false) // Synthetic qualification callback is never an executed provider.
}
#[test]
fn synthetic_raw_pass_materializes_and_preserves_original_without_inventing_request() {
    let f = fixture();
    let (admission, recipe, _, _) = inputs(&f);
    let execution = execution(&f);
    let before = fs::read(f.file("original_request")).unwrap();
    let output = f.dir.path().join("generated");
    let calls = Cell::new(0);
    let report = generate_with(&admission, &recipe, &execution, &output, |inv| {
        calls.set(calls.get() + 1);
        #[cfg(windows)]
        {
            assert!(fs::write(f.dir.path().join("model.bin"), b"changed").is_err());
            assert!(fs::write(inv.output.join("patch-source-0.wav"), b"changed").is_err());
        }
        fake(inv, 0)
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(report["provider_process_executed"], false);
    assert_eq!(report["outside_mask_sample_bits_exact"], true);
    assert_eq!(report["performed_words_verified"], false);
    assert!(!output.join("request.json").exists());
    assert_eq!(before, fs::read(f.file("original_request")).unwrap());
    let receipt =
        packet::parse(&fs::read(output.join("generation-receipt.json")).unwrap()).unwrap();
    assert_eq!(
        receipt["raw_candidate_normalization"][0]["truncated_tail_frames"],
        100
    );
    assert_eq!(receipt["human_listening"], "pending");
    assert_eq!(receipt["selected"], false);
    check(
        &admission,
        &recipe,
        &output.join("candidates.json"),
        &output.join("materialized"),
    )
    .unwrap();
    assert!(
        generate_with(&admission, &recipe, &execution, &output, |_| panic!(
            "existing destination must refuse"
        ))
        .is_err()
    );
}
#[test]
fn invalid_execution_or_missing_installed_bytes_refuse_before_callback_or_output() {
    for mutation in 0..8 {
        let f = fixture();
        let (admission, recipe, _, _) = inputs(&f);
        let execution = execution(&f);
        let mut e = packet::parse(&fs::read(&execution).unwrap()).unwrap();
        match mutation {
            0 => e["permissions"]["public_release"] = json!(true),
            1 => e["timeout_seconds"] = json!(3601),
            2 => e["processes"] = json!(0),
            3 => e["seed"] = json!(u32::MAX),
            4 => e["repaint_strength"] = json!(2),
            5 => {
                fs::write(f.dir.path().join("model.bin"), b"changed model").unwrap();
            }
            6 => e["backend"]["path"] = json!("../local_song_repaint.py"),
            _ => {
                let path = f.dir.path().join("runtime-input.json");
                let mut runtime = packet::parse(&fs::read(&path).unwrap()).unwrap();
                runtime["generation_files"] = json!([]);
                save(&path, &runtime);
                e["runtime"]["object"] = descriptor(&path);
            }
        }
        save(&execution, &e);
        let output = f.dir.path().join("refused");
        let calls = Cell::new(0);
        assert!(
            generate_with(&admission, &recipe, &execution, &output, |_| {
                calls.set(calls.get() + 1);
                Ok(false)
            })
            .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(calls.get(), 0);
        assert!(!output.exists());
    }
}
#[test]
fn bad_attempt_receipt_raw_clock_or_signal_does_not_retry_or_materialize() {
    for mutation in 1..=5 {
        let f = fixture();
        let (admission, recipe, _, _) = inputs(&f);
        let execution = execution(&f);
        let output = f.dir.path().join("failed");
        let calls = Cell::new(0);
        assert!(
            generate_with(&admission, &recipe, &execution, &output, |inv| {
                calls.set(calls.get() + 1);
                fake(inv, mutation)
            })
            .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(calls.get(), 1);
        assert!(output.join("provider-receipt.json").exists());
        assert!(!output.join("materialized").exists());
    }
}

#[test]
#[cfg(windows)]
#[ignore = "requires explicit installed REEL_TEST_PYTHON; synthetic audio only, no ACE"]
fn explicit_python_child_obeys_native_input_guards_and_materializes_synthetic_audio() {
    let python = PathBuf::from(
        std::env::var_os("REEL_TEST_PYTHON").expect("explicit installed Python required"),
    );
    assert!(python.is_absolute());
    let f = fixture();
    let (admission, recipe, _, _) = inputs(&f);
    let execution = execution(&f);
    let script = r#"import hashlib,json,struct
from pathlib import Path
def load(name): return json.loads(Path(name).read_text(encoding='utf-8'))
def pin(name):
    b=Path(name).read_bytes()
    return dict(sha256=hashlib.sha256(b).hexdigest(),bytes=len(b))
request=load('provider-request.json')
runtime=load('runtime.json')
for name in ['patch-source-0.wav','source-context.json',runtime['generation_files'][0]['path']]:
    try:
        with open(name,'r+b'): pass
    except PermissionError: pass
    else: raise RuntimeError('native input write guard did not exclude the child')
attempts=[]
for index,p in enumerate(request['patches']):
    frames=p['context_end_frame']-p['context_start_frame']
    rate=request['sample_rate'];channels=request['channels']
    data=struct.pack('<ff',.5,-.5)*frames
    raw=b'RIFF'+struct.pack('<I',len(data)+36)+b'WAVEfmt '+struct.pack('<IHHIIHH',16,3,channels,rate,rate*channels*4,channels*4,32)+b'data'+struct.pack('<I',len(data))+data
    name=f'raw-patch-{index}.wav'
    with open(name,'xb') as stream: stream.write(raw)
    attempts.append(dict(index=index,attempt=1,seed=request['seed']+index,context_start_frame=p['context_start_frame'],context_end_frame=p['context_end_frame'],context_audio=p['context_audio'],raw_audio=pin(name)))
with open('provider-receipt.json','x',encoding='utf-8') as stream:
    json.dump(dict(schema='reel.original-song-repair-provider-receipt.v1',request=pin('provider-request.json'),runtime=pin('runtime.json'),source_context=pin('source-context.json'),patches=attempts,external_upload=False,human_approval=False,selected=False),stream)
"#;
    let backend = f.dir.path().join("local_song_repaint.py");
    fs::write(&backend, script).unwrap();
    let runtime_path = f.dir.path().join("runtime-input.json");
    let mut runtime = packet::parse(&fs::read(&runtime_path).unwrap()).unwrap();
    let mut python_pin = descriptor(&python);
    python_pin["path"] = json!(python);
    runtime["python"] = python_pin;
    save(&runtime_path, &runtime);
    let mut e = packet::parse(&fs::read(&execution).unwrap()).unwrap();
    e["runtime"]["object"] = descriptor(&runtime_path);
    e["backend"]["object"] = descriptor(&backend);
    save(&execution, &e);
    let output = f.dir.path().join("actual-contained-child");
    let report = generate(&admission, &recipe, &execution, &output).unwrap();
    assert_eq!(report["provider_process_executed"], true);
    assert_eq!(report["outside_mask_sample_bits_exact"], true);
    assert_eq!(report["human_approval"], false);
    check(
        &admission,
        &recipe,
        &output.join("candidates.json"),
        &output.join("materialized"),
    )
    .unwrap();
    let restored = f.dir.path().join("restored-contained-child");
    fs::rename(&output, &restored).unwrap();
    check(
        &admission,
        &recipe,
        &restored.join("candidates.json"),
        &restored.join("materialized"),
    )
    .unwrap();
}
