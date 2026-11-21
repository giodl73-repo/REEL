//! One explicit POST per invocation; no cloning, sharing, automatic retry or selection.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;
const CAP: usize = 16 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    path: PathBuf,
    sha256: String,
    bytes: usize,
}
#[derive(Deserialize)]
#[serde(tag = "schema", deny_unknown_fields)]
enum Request {
    #[serde(rename = "reel.voice-design-request.v1")]
    Design {
        role_id: String,
        description: String,
        text: String,
        model_id: String,
        seed: u32,
        authorization: Pin,
        generation_authorized: bool,
        public_release: bool,
        timeout_seconds: u64,
    },
    #[serde(rename = "reel.voice-save-request.v1")]
    Save {
        role_id: String,
        voice_name: String,
        description: String,
        design_response: Pin,
        candidate_index: usize,
        authorization: Pin,
        generation_authorized: bool,
        public_release: bool,
        timeout_seconds: u64,
    },
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn read(path: &Path, cap: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    fs::File::open(path)?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= cap, "Input exceeds budget");
    Ok(bytes)
}
fn pinned(pin: &Pin, cap: usize) -> Result<Vec<u8>> {
    ensure!(
        pin.path.is_absolute() && pin.bytes <= cap,
        "Invalid explicit input pin"
    );
    let bytes = read(&pin.path, pin.bytes)?;
    ensure!(
        bytes.len() == pin.bytes && hash(&bytes) == pin.sha256,
        "Input pin mismatch"
    );
    Ok(bytes)
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn new_file(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn prepare(request: &Request) -> Result<(&str, Value, Vec<u8>, u64)> {
    let (role, description, authorization, authorized, public, timeout) = match request {
        Request::Design {
            role_id,
            description,
            authorization,
            generation_authorized,
            public_release,
            timeout_seconds,
            ..
        }
        | Request::Save {
            role_id,
            description,
            authorization,
            generation_authorized,
            public_release,
            timeout_seconds,
            ..
        } => (
            role_id,
            description,
            authorization,
            generation_authorized,
            public_release,
            timeout_seconds,
        ),
    };
    ensure!(
        token(role)
            && (20..=1000).contains(&description.chars().count())
            && *authorized
            && !public
            && (1..=180).contains(timeout),
        "Request scope refused"
    );
    let evidence = pinned(authorization, 65536)?;
    let scope: Value = serde_json::from_slice(&evidence)?;
    ensure!(
        scope["generation_authorized"] == true
            && scope["public_release"] == false
            && scope["synthetic_original_only"] == true
            && scope["roles"]
                .as_array()
                .is_some_and(|r| r.iter().any(|v| v.as_str() == Some(role))),
        "Authorization does not name this original synthetic role"
    );
    match request {
        Request::Design {
            text,
            model_id,
            seed,
            ..
        } => {
            ensure!(
                (100..=1000).contains(&text.chars().count())
                    && *seed <= i32::MAX as u32
                    && matches!(
                        model_id.as_str(),
                        "eleven_multilingual_ttv_v2" | "eleven_ttv_v3"
                    ),
                "Design parameters refused"
            );
            Ok((
                "https://api.elevenlabs.io/v1/text-to-voice/design?output_format=mp3_44100_128",
                json!({"voice_description":description,"text":text,"auto_generate_text":false,"model_id":model_id,"seed":seed,"guidance_scale":5,"stream_previews":false,"should_enhance":false}),
                evidence,
                *timeout,
            ))
        }
        Request::Save {
            voice_name,
            design_response,
            candidate_index,
            ..
        } => {
            ensure!(
                !voice_name.trim().is_empty()
                    && voice_name.chars().count() <= 100
                    && (1..=3).contains(candidate_index),
                "Save parameters refused"
            );
            let raw = pinned(design_response, CAP)?;
            let previews: Value = serde_json::from_slice(&raw)?;
            let id = previews["previews"]
                .get(candidate_index - 1)
                .and_then(|v| v["generated_voice_id"].as_str())
                .context("Preview missing")?;
            ensure!(token(id), "Invalid generated ID");
            Ok((
                "https://api.elevenlabs.io/v1/text-to-voice",
                json!({"voice_name":voice_name,"voice_description":description,"generated_voice_id":id}),
                evidence,
                *timeout,
            ))
        }
    }
}
fn materialize(request: &Request, raw: &[u8], root: &Path) -> Result<Value> {
    let value: Value = serde_json::from_slice(raw)?;
    let mut outputs = vec![];
    match request {
        Request::Design { .. } => {
            let previews = value["previews"].as_array().context("Missing previews")?;
            ensure!(
                !previews.is_empty() && previews.len() <= 3,
                "Unexpected preview count"
            );
            for (i, p) in previews.iter().enumerate() {
                ensure!(
                    p["generated_voice_id"].as_str().is_some_and(token),
                    "Invalid preview identity"
                );
                let audio = STANDARD.decode(
                    p["audio_base_64"]
                        .as_str()
                        .context("Missing preview audio")?,
                )?;
                ensure!(
                    !audio.is_empty() && audio.len() <= 8 * 1024 * 1024,
                    "Preview audio exceeds budget"
                );
                ensure!(
                    p["media_type"]
                        .as_str()
                        .is_some_and(|s| s.contains("mpeg") || s.contains("mp3")),
                    "Unexpected preview codec"
                );
                let name = format!("preview-{}.mp3", i + 1);
                new_file(root, &name, &audio)?;
                outputs.push(json!({"path":name,"sha256":hash(&audio),"bytes":audio.len(),"duration_secs":p["duration_secs"],"generated_voice_id":p["generated_voice_id"]}));
            }
        }
        Request::Save { .. } => {
            ensure!(
                value["voice_id"].as_str().is_some_and(token),
                "Missing saved voice ID"
            );
            ensure!(
                !matches!(value["sharing"]["status"].as_str(), Some("enabled")),
                "Provider returned a shared voice; inspect private response"
            );
            outputs.push(
                json!({"voice_id":value["voice_id"],"name":value["name"],"candidate_only":true}),
            );
        }
    }
    let receipt = json!({"schema":"reel.voice-design-receipt.v1","provider":"elevenlabs","native_http_attempts":1,"response":{"sha256":hash(raw),"bytes":raw.len()},"outputs":outputs,"synthetic_original_only":true,"production_selection":false,"human_approval":false,"public_release":false});
    new_file(root, "receipt.json", &serde_json::to_vec_pretty(&receipt)?)?;
    Ok(receipt)
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() == 3 && args[0] == "review" {
        return review(Path::new(&args[1]), Path::new(&args[2]));
    }
    ensure!(
        args.len() >= 2 && matches!(args[0].as_str(), "inspect" | "execute"),
        "Usage: reel-voice-design inspect REQUEST | execute REQUEST NEW_ABSOLUTE_DIRECTORY"
    );
    let raw = read(Path::new(&args[1]), 65536)?;
    let request: Request = serde_json::from_slice(&raw)?;
    let (url, body, evidence, timeout) = prepare(&request)?;
    if args[0] == "inspect" {
        ensure!(args.len() == 2, "Invalid arguments");
        println!("{}", json!({"valid":true,"provider_calls":0}));
        return Ok(());
    }
    ensure!(args.len() == 3, "Missing new output directory");
    let root = Path::new(&args[2]);
    ensure!(root.is_absolute(), "Absolute output required");
    let key = Zeroizing::new(env::var("ELEVENLABS_API_KEY").context("Credential unavailable")?);
    ensure!(!key.trim().is_empty(), "Empty credential");
    let bytes = serde_json::to_vec(&body)?;
    ensure!(
        !raw.windows(key.len()).any(|v| v == key.as_bytes())
            && !bytes.windows(key.len()).any(|v| v == key.as_bytes()),
        "Credential reflection refused"
    );
    fs::create_dir(root)?;
    new_file(root, "request.json", &raw)?;
    new_file(root, "authorization.json", &evidence)?;
    new_file(
        root,
        "attempt.json",
        br#"{"http_attempts":1,"state":"started-no-automatic-retry"}"#,
    )?;
    let result = (|| -> Result<Value> {
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(timeout)))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let mut response = agent
            .post(url)
            .header("xi-api-key", key.as_str())
            .header("Content-Type", "application/json")
            .send(&bytes)?;
        let status = response.status().as_u16();
        let mut response_bytes = vec![];
        response
            .body_mut()
            .as_reader()
            .take(CAP as u64 + 1)
            .read_to_end(&mut response_bytes)?;
        ensure!(
            response_bytes.len() <= CAP
                && !response_bytes
                    .windows(key.len())
                    .any(|v| v == key.as_bytes()),
            "Response budget or credential reflection refused"
        );
        new_file(
            root,
            "http.json",
            &serde_json::to_vec(&json!({"status":status}))?,
        )?;
        new_file(root, "response.json", &response_bytes)?;
        ensure!(status == 200, "Provider refused; inspect private evidence");
        materialize(&request, &response_bytes, root)
    })();
    match result {
        Ok(receipt) => println!(
            "{}",
            json!({"succeeded":true,"outputs":receipt["outputs"].as_array().map(Vec::len),"candidate_only":true})
        ),
        Err(error) => {
            let _ = new_file(
                root,
                "failure.json",
                br#"{"state":"failed-or-indeterminate-no-automatic-retry","human_approval":false}"#,
            );
            return Err(error);
        }
    };
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewItem {
    id: String,
    character: String,
    kind: String,
    language: String,
    text: String,
    source: Pin,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    schema: String,
    title: String,
    notice: String,
    items: Vec<ReviewItem>,
}
fn review(path: &Path, output: &Path) -> Result<()> {
    let request: Review = serde_json::from_slice(&read(path, 65536)?)?;
    ensure!(
        request.schema == "reel.voice-audition-review.v1"
            && (1..=64).contains(&request.items.len())
            && output.is_absolute(),
        "Invalid audition review"
    );
    let mut audios = std::collections::BTreeMap::new();
    let mut items = vec![];
    let mut ids = std::collections::BTreeSet::new();
    let mut total = 0;
    for item in request.items {
        ensure!(
            token(&item.id) && ids.insert(item.id.clone()),
            "Duplicate/invalid audition item"
        );
        let extension = item
            .source
            .path
            .extension()
            .and_then(|e| e.to_str())
            .context("Missing audio extension")?;
        ensure!(
            matches!(extension, "wav" | "mp3"),
            "Unsupported audition audio"
        );
        let bytes = pinned(&item.source, 8 * 1024 * 1024)?;
        let src = format!("media/{}.{}", item.source.sha256, extension);
        if !audios.contains_key(&src) {
            total += bytes.len();
            ensure!(total <= 256 * 1024 * 1024, "Review exceeds audio budget");
            audios.insert(src.clone(), bytes);
        }
        items.push(json!({"id":item.id,"character":item.character,"kind":item.kind,"language":item.language,"text":item.text,"src":src,"sha256":item.source.sha256,"bytes":item.source.bytes}));
    }
    let data = serde_json::to_vec(
        &json!({"schema":"reel.voice-audition-session.v1","title":request.title,"notice":request.notice,"audios":items}),
    )?;
    ensure!(
        data.len() <= 2 * 1024 * 1024,
        "Review control exceeds budget"
    );
    fs::create_dir(output)?;
    fs::create_dir(output.join("media"))?;
    new_file(output, "index.html", include_bytes!("review.html"))?;
    new_file(output, "editor.js", include_bytes!("review.js"))?;
    for (name, bytes) in audios {
        new_file(output, &name, &bytes)?;
    }
    new_file(output, "session.json", &data)?;
    println!(
        "{}",
        json!({"items":items.len(),"audio_bytes":total,"provider_calls":0,"human_approval":false})
    );
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!(
            "Voice design refused; inspect exact private request/attempt. No automatic retry."
        );
        std::process::exit(1)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_pin_and_no_clobber() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x");
        fs::write(&p, b"abc").unwrap();
        let pin = Pin {
            path: p,
            sha256: hash(b"abc"),
            bytes: 3,
        };
        assert_eq!(pinned(&pin, 3).unwrap(), b"abc");
        assert!(pinned(&pin, 2).is_err());
        new_file(d.path(), "y", b"x").unwrap();
        assert!(new_file(d.path(), "y", b"z").is_err());
    }
    #[test]
    fn scope_and_private_defaults_refuse() {
        let d = tempfile::tempdir().unwrap();
        let data=br#"{"generation_authorized":true,"public_release":false,"synthetic_original_only":true,"roles":["one"]}"#;
        let p = d.path().join("scope.json");
        fs::write(&p, data).unwrap();
        let mut value = json!({"schema":"reel.voice-design-request.v1","role_id":"one","description":"Original adult character with an intimate natural speaking voice.","text":"A".repeat(100),"model_id":"eleven_multilingual_ttv_v2","seed":1,"authorization":{"path":p,"sha256":hash(data),"bytes":data.len()},"generation_authorized":true,"public_release":false,"timeout_seconds":30});
        assert!(prepare(&serde_json::from_value::<Request>(value.clone()).unwrap()).is_ok());
        value["role_id"] = json!("unlisted");
        assert!(prepare(&serde_json::from_value::<Request>(value.clone()).unwrap()).is_err());
        value["role_id"] = json!("one");
        value["public_release"] = json!(true);
        assert!(prepare(&serde_json::from_value::<Request>(value).unwrap()).is_err());
    }
    #[test]
    fn previews_capture_audio_without_approval() {
        let d = tempfile::tempdir().unwrap();
        let request:Request=serde_json::from_value(json!({"schema":"reel.voice-design-request.v1","role_id":"one","description":"Original adult character voice.","text":"A".repeat(100),"model_id":"eleven_multilingual_ttv_v2","seed":1,"authorization":{"path":"unused","sha256":"unused","bytes":0},"generation_authorized":true,"public_release":false,"timeout_seconds":30})).unwrap();
        let raw=serde_json::to_vec(&json!({"previews":[{"generated_voice_id":"sample","audio_base_64":STANDARD.encode(b"synthetic-test-fixture"),"media_type":"audio/mpeg","duration_secs":1.0}]})).unwrap();
        let r = materialize(&request, &raw, d.path()).unwrap();
        assert_eq!(r["human_approval"], false);
        assert_eq!(
            fs::read(d.path().join("preview-1.mp3")).unwrap(),
            b"synthetic-test-fixture"
        );
    }
    #[test]
    fn review_freezes_named_audio_and_refuses_changed_source_or_existing_output() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("one.wav");
        fs::write(&source, b"synthetic fixture").unwrap();
        let request = json!({"schema":"reel.voice-audition-review.v1","title":"Synthetic","notice":"Test","items":[{"id":"one","character":"Actor","kind":"audition","language":"en","text":"Test","source":{"path":source,"sha256":hash(b"synthetic fixture"),"bytes":17}}]});
        let path = d.path().join("request.json");
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        let output = d.path().join("review");
        review(&path, &output).unwrap();
        assert!(review(&path, &output).is_err());
        fs::write(source, b"changed").unwrap();
        let failed = d.path().join("failed");
        assert!(review(&path, &failed).is_err());
        assert!(!failed.exists());
    }
}
