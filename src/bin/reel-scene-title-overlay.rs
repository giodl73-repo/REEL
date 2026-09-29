//! Apply an editable title template to the opening picture of a target scene.
//! The presentation scene declares placement; the target scene owns the picture.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRef {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    language: String,
    presentation_scene: FileRef,
    target_scene: FileRef,
    template: FileRef,
    layer: FileRef,
    base_scene: FileRef,
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    schema: String,
    language: String,
    presentation_scene_id: String,
    target_scene_id: String,
    target_picture_slot_id: String,
    template_id: String,
    manifest_sha256: String,
    base_sha256: String,
    review_sha256: String,
    review_bytes: u64,
    frames: u64,
    publication: String,
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn verified(root: &Path, r: &FileRef) -> Result<Vec<u8>> {
    let path = root.join(&r.path);
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() as u64 != r.bytes || hash(&bytes) != r.sha256 {
        bail!("input hash/bytes mismatch: {}", path.display());
    }
    Ok(bytes)
}

fn output(mut cmd: Command) -> Result<Vec<u8>> {
    let result = cmd.output()?;
    if !result.status.success() {
        bail!(
            "media command failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(result.stdout)
}

fn media(path: &Path) -> Result<serde_json::Value> {
    let mut cmd = Command::new("ffprobe");
    cmd.args([
        "-v",
        "error",
        "-count_frames",
        "-show_streams",
        "-of",
        "json",
    ])
    .arg(path);
    Ok(serde_json::from_slice(&output(cmd)?)?)
}

fn frame_count(value: &serde_json::Value) -> Result<u64> {
    value["streams"]
        .as_array()
        .context("no streams")?
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .and_then(|stream| stream["nb_read_frames"].as_str())
        .and_then(|count| count.parse::<u64>().ok())
        .context("no video frame count")
}

fn audio_hash(path: &Path) -> Result<Vec<u8>> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-i"]).arg(path).args([
        "-map", "0:a:0", "-c", "copy", "-f", "hash", "-hash", "SHA256", "-",
    ]);
    output(cmd)
}

fn inputs(
    manifest: &Manifest,
    root: &Path,
) -> Result<(serde_json::Value, serde_json::Value, serde_json::Value)> {
    if manifest.schema != "reel.scene-title-overlay.v1"
        || !["es", "en"].contains(&manifest.language.as_str())
    {
        bail!("invalid title-overlay manifest");
    }
    let presentation: serde_json::Value =
        serde_json::from_slice(&verified(root, &manifest.presentation_scene)?)?;
    let target: serde_json::Value =
        serde_json::from_slice(&verified(root, &manifest.target_scene)?)?;
    let template: serde_json::Value = serde_json::from_slice(&verified(root, &manifest.template)?)?;
    let layer = verified(root, &manifest.layer)?;
    verified(root, &manifest.base_scene)?;
    let placement = &presentation["presentation"]["placement"];
    if placement["mode"] != "overlay-on-target-scene-start"
        || placement["target_scene_id"] != target["scene_id"]
        || presentation["presentation"]["template_id"] != template["template_id"]
        || template["kind"] != "chapter-title"
        || template["fixed_duration_seconds"].as_u64() != Some(4)
    {
        bail!("title placement/template contract mismatch");
    }
    let first = target["language_event_bindings"][&manifest.language]
        .as_array()
        .and_then(|events| events.first())
        .context("target has no first language event")?;
    if first["picture_slot_id"] != placement["target_picture_slot_id"] {
        bail!("title does not bind first picture slot");
    }
    let ass = String::from_utf8(layer)?;
    if !ass.contains("0:00:00.00,0:00:04.00") {
        bail!("title layer must run over the first four seconds");
    }
    Ok((presentation, target, template))
}

fn build(manifest_path: &Path, root: &Path, out: &Path) -> Result<()> {
    if out.exists() {
        bail!("output directory already exists");
    }
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    let (presentation, target, template) = inputs(&manifest, root)?;
    fs::create_dir_all(out)?;
    let out = fs::canonicalize(out)?;
    fs::write(out.join("layer.ass"), verified(root, &manifest.layer)?)?;
    let base = root.join(&manifest.base_scene.path);
    let review = out.join("review.mp4");
    let mut cmd = Command::new("ffmpeg");
    cmd.current_dir(&out)
        .args(["-y", "-v", "error", "-i"])
        .arg(&base)
        .args([
            "-vf",
            "ass=layer.ass",
            "-map",
            "0:v:0",
            "-map",
            "0:a:0",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "16",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "copy",
        ])
        .arg(&review);
    output(cmd)?;
    let base_facts = media(&base)?;
    let review_facts = media(&review)?;
    let frames = frame_count(&base_facts)?;
    if frame_count(&review_facts)? != frames || audio_hash(&base)? != audio_hash(&review)? {
        bail!("scene picture/audio closure mismatch");
    }
    let mut decode = Command::new("ffmpeg");
    decode
        .args(["-v", "error", "-i"])
        .arg(&review)
        .args(["-f", "null", "-"]);
    output(decode)?;
    let bytes = fs::read(&review)?;
    let receipt = Receipt {
        schema: "reel.scene-title-overlay-receipt.v1".into(),
        language: manifest.language,
        presentation_scene_id: presentation["scene_id"].as_str().unwrap().into(),
        target_scene_id: target["scene_id"].as_str().unwrap().into(),
        target_picture_slot_id: presentation["presentation"]["placement"]["target_picture_slot_id"]
            .as_str()
            .unwrap()
            .into(),
        template_id: template["template_id"].as_str().unwrap().into(),
        manifest_sha256: hash(&manifest_bytes),
        base_sha256: manifest.base_scene.sha256,
        review_sha256: hash(&bytes),
        review_bytes: bytes.len() as u64,
        frames,
        publication: "not-authorized".into(),
    };
    fs::write(
        out.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!("{} {} frames", receipt.language, frames);
    Ok(())
}

fn check(manifest_path: &Path, root: &Path, out: &Path) -> Result<()> {
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    inputs(&manifest, root)?;
    let receipt: Receipt = serde_json::from_slice(&fs::read(out.join("receipt.json"))?)?;
    let review = out.join("review.mp4");
    let bytes = fs::read(&review)?;
    let base = root.join(&manifest.base_scene.path);
    if receipt.schema != "reel.scene-title-overlay-receipt.v1"
        || receipt.manifest_sha256 != hash(&manifest_bytes)
        || receipt.base_sha256 != manifest.base_scene.sha256
        || receipt.review_sha256 != hash(&bytes)
        || receipt.review_bytes != bytes.len() as u64
        || receipt.frames != frame_count(&media(&review)?)?
        || receipt.frames != frame_count(&media(&base)?)?
        || audio_hash(&base)? != audio_hash(&review)?
    {
        bail!("title overlay receipt/output closure mismatch");
    }
    let mut decode = Command::new("ffmpeg");
    decode
        .args(["-v", "error", "-i"])
        .arg(&review)
        .args(["-f", "null", "-"]);
    output(decode)?;
    println!("scene title overlay verified");
    Ok(())
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let result = (|| -> Result<()> {
        if let [_, action, manifest, root_flag, root, output_flag, out] = args.as_slice() {
            if root_flag != "--root" || output_flag != "--output-dir" {
                bail!("invalid flags")
            } else {
                match action.as_str() {
                    "build" => build(Path::new(manifest), Path::new(root), Path::new(out)),
                    "check" => check(Path::new(manifest), Path::new(root), Path::new(out)),
                    _ => bail!("expected build or check"),
                }
            }
        } else {
            bail!(
                "usage: reel-scene-title-overlay <build|check> <manifest> --root <root> --output-dir <dir>"
            )
        }
    })();
    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
