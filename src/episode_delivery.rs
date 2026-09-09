//! Verify the decoded contents of a lossless episode against ordered scene deliveries.
use crate::scene_delivery::{self, FileRef};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Seek},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub id: String,
    pub job: FileRef,
    pub asset_root: PathBuf,
    pub receipt: FileRef,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub schema: String,
    pub id: String,
    pub scenes: Vec<Scene>,
    pub master: FileRef,
    pub layers: Vec<Layer>,
    pub boundary_decisions: Vec<BoundaryDecision>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub scene_id: String,
    pub attachment_id: String,
    /// baked-picture or excluded-from-clean-master. No implicit inclusion.
    pub disposition: String,
    pub picture_attachment_id: Option<String>,
    pub evidence: FileRef,
    pub reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryDecision {
    pub left_scene: String,
    pub right_scene: String,
    pub left_receipt_sha256: String,
    pub right_receipt_sha256: String,
    pub code: String,
    pub owner: String,
    pub reason: String,
    pub evidence: FileRef,
}

#[derive(Debug, Serialize)]
pub struct BoundaryFinding {
    pub left_scene: String,
    pub right_scene: String,
    pub code: String,
    pub measured: f64,
    pub threshold: f64,
    pub disposition: String,
}

#[derive(Debug, Serialize)]
pub struct Consumption {
    pub scene_id: String,
    pub receipt_sha256: String,
    pub job_sha256: String,
    pub start_frame: u64,
    pub start_sample: u64,
    pub frames: u64,
    pub samples: u64,
    pub picture_sha256: String,
    pub mix_sha256: String,
    pub decoded_picture_sha256: String,
    pub decoded_mix_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: String,
    pub contract_sha256: String,
    pub master_sha256: String,
    pub content_verified: bool,
    pub passed: bool,
    pub scenes: Vec<Consumption>,
    pub boundary_findings: Vec<BoundaryFinding>,
    pub layers: Vec<Layer>,
    pub limitations: Vec<String>,
}

pub(crate) fn local_dir(root: &Path, path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!("directory must be a nonempty relative local path");
    }
    let root = root.canonicalize()?;
    let mut exact = root.clone();
    for c in path.components() {
        if !fs::read_dir(&exact)?.any(|e| e.is_ok_and(|e| e.file_name() == c.as_os_str())) {
            bail!("missing or case-mismatched directory");
        }
        exact.push(c.as_os_str());
    }
    let p = exact.canonicalize()?;
    if !p.starts_with(root) || !p.is_dir() {
        bail!("directory escapes root");
    }
    Ok(p)
}

pub(crate) fn load_scene(
    root: &Path,
    scene: &Scene,
) -> Result<(scene_delivery::Job, scene_delivery::Receipt, PathBuf)> {
    if scene.id.trim().is_empty() {
        bail!("empty scene id");
    }
    let job = scene_delivery::checked_file(root, &scene.job)?;
    let receipt = scene_delivery::checked_file(root, &scene.receipt)?;
    if receipt.file_name().and_then(|s| s.to_str()) != Some("receipt.json") {
        bail!("scene receipt must be receipt.json");
    }
    let output = receipt.parent().context("receipt directory")?.to_path_buf();
    let assets = local_dir(root, &scene.asset_root)?;
    let r = scene_delivery::check(&job, &assets, &output)?;
    let j: scene_delivery::Job = serde_yaml::from_slice(&fs::read(job)?)?;
    if r.plan.id != scene.id {
        bail!("scene id differs from compiled scene");
    }
    Ok((j, r, output))
}

/// Hash each segment in one bounded-memory decode. Exact byte counts reject
/// extra/short media; no timestamp seek, rate conversion or frame interpolation.
pub(crate) fn decode_segments(path: &Path, video: bool, lengths: &[u64]) -> Result<Vec<String>> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-v",
        "error",
        "-nostdin",
        "-protocol_whitelist",
        "file,pipe",
        "-i",
    ])
    .arg(path);
    if video {
        cmd.args([
            "-map",
            "0:v:0",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "yuv444p",
            "-f",
            "rawvideo",
        ]);
    } else {
        cmd.args(["-map", "0:a:0", "-c:a", "pcm_s24le", "-f", "s24le"]);
    }
    let mut errors = tempfile::tempfile()?;
    let mut child = cmd
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(errors.try_clone()?)
        .spawn()?;
    let result = (|| -> Result<Vec<String>> {
        let mut pipe = child.stdout.take().context("decoder pipe")?;
        let mut hashes = Vec::new();
        let mut buffer = [0u8; 65536];
        for &length in lengths {
            let mut remaining = length;
            let mut hash = Sha256::new();
            while remaining > 0 {
                let n = usize::try_from(remaining.min(buffer.len() as u64))?;
                pipe.read_exact(&mut buffer[..n])
                    .context("decoded segment is short")?;
                hash.update(&buffer[..n]);
                remaining -= n as u64;
            }
            hashes.push(hash.finalize().iter().map(|b| format!("{b:02x}")).collect());
        }
        if pipe.read(&mut [0u8; 1])? != 0 {
            bail!("decoded media has extra content");
        }
        Ok(hashes)
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    errors.rewind()?;
    let mut diagnostics = String::new();
    errors.take(8192).read_to_string(&mut diagnostics)?;
    if !status.success() && result.is_ok() {
        bail!("media decode failed");
    }
    result.with_context(|| {
        format!(
            "decode failed for {} ({}) {diagnostics}",
            path.display(),
            if video { "picture" } else { "audio" }
        )
    })
}

fn edge_audio(path: &Path, samples: u64, sr: u32, end: bool) -> Result<Vec<f32>> {
    let window = u64::from((sr / 10).max(1)).min(samples);
    let start = if end { samples - window } else { 0 };
    let o = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-af",
            &format!("atrim=start_sample={start}:end_sample={}", start + window),
            "-f",
            "f32le",
            "-c:a",
            "pcm_f32le",
            "-",
        ])
        .output()?;
    if !o.status.success() || o.stdout.len() as u64 != window * 8 {
        bail!("audio edge decode failed");
    }
    Ok(o.stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect())
}

fn black_edge(path: &Path, frame: u64, width: u32, height: u32) -> Result<bool> {
    let o = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args([
            "-map",
            "0:v:0",
            "-vf",
            &format!("select=eq(n\\,{frame})"),
            "-frames:v",
            "1",
            "-pix_fmt",
            "gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()?;
    if !o.status.success() || o.stdout.len() as u64 != u64::from(width) * u64::from(height) {
        bail!("picture edge decode failed");
    }
    Ok(o.stdout.iter().filter(|&&x| x <= 16).count() as f64 / o.stdout.len() as f64 >= 0.98)
}

fn rms_db(values: &[f32]) -> f64 {
    let power =
        values.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / values.len().max(1) as f64;
    10.0 * power.max(1e-12).log10()
}

fn verify_stem_sum(dir: &Path, samples: u64) -> Result<()> {
    let mut children = Vec::new();
    let result = (|| -> Result<()> {
        for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
            children.push(
                Command::new("ffmpeg")
                    .args([
                        "-v",
                        "error",
                        "-nostdin",
                        "-protocol_whitelist",
                        "file,pipe",
                        "-i",
                    ])
                    .arg(dir.join(name))
                    .args(["-map", "0:a:0", "-c:a", "pcm_f32le", "-f", "f32le", "-"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()?,
            );
        }
        let mut pipes = children
            .iter_mut()
            .map(|c| c.stdout.take().context("stem decode pipe"))
            .collect::<Result<Vec<_>>>()?;
        let mut remaining = samples.checked_mul(8).context("stem size overflow")?;
        let mut buffers = vec![vec![0u8; 32768]; 4];
        while remaining > 0 {
            let n = remaining.min(32768) as usize;
            for (pipe, buffer) in pipes.iter_mut().zip(&mut buffers) {
                pipe.read_exact(&mut buffer[..n])?;
            }
            for offset in (0..n).step_by(4) {
                let v: [f64; 4] = std::array::from_fn(|i| {
                    f64::from(f32::from_le_bytes(
                        buffers[i][offset..offset + 4].try_into().unwrap(),
                    ))
                });
                if v.iter().any(|x| !x.is_finite())
                    || (v[0] + v[1] + v[2] - v[3]).abs() > 3.0 / 8_388_608.0
                {
                    bail!("scene mix does not recombine D/M/E within PCM24 quantization tolerance");
                }
            }
            remaining -= n as u64;
        }
        for pipe in &mut pipes {
            if pipe.read(&mut [0u8; 1])? != 0 {
                bail!("extra stem samples");
            }
        }
        Ok(())
    })();
    if result.is_err() {
        for child in &mut children {
            let _ = child.kill();
        }
    }
    let mut success = true;
    for child in &mut children {
        success &= child.wait()?.success();
    }
    result?;
    if !success {
        bail!("stem decode failed");
    }
    Ok(())
}

fn boundary_findings(
    left: &str,
    right: &str,
    jump: f64,
    ambience: f64,
    tail: f64,
    black: bool,
) -> Vec<BoundaryFinding> {
    [
        ("audio-step", jump, 0.1),
        ("ambience-level-step", ambience, 12.0),
        ("music-tail-risk", tail, 0.01),
        ("black-at-cut", if black { 1.0 } else { 0.0 }, 0.0),
    ]
    .into_iter()
    .filter(|(_, v, t)| v > t)
    .map(|(code, measured, threshold)| BoundaryFinding {
        left_scene: left.into(),
        right_scene: right.into(),
        code: code.into(),
        measured,
        threshold,
        disposition: "needs-review".into(),
    })
    .collect()
}

fn verify_timestamps(path: &Path, sr: u32, numerator: u64, denominator: u64) -> Result<()> {
    let o = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_frames",
            "-show_entries",
            "frame=media_type,best_effort_timestamp_time,nb_samples",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !o.status.success() {
        bail!("master timestamp decode failed");
    }
    let data: serde_json::Value = serde_json::from_slice(&o.stdout)?;
    let frames = data["frames"]
        .as_array()
        .context("master timestamps missing")?;
    let (mut video, mut audio) = (0u64, 0u64);
    for frame in frames {
        let time = frame["best_effort_timestamp_time"]
            .as_str()
            .context("frame timestamp missing")?
            .parse::<f64>()?;
        let expected = match frame["media_type"].as_str() {
            Some("video") => {
                let t = video as f64 * denominator as f64 / numerator as f64;
                video += 1;
                t
            }
            Some("audio") => {
                let t = audio as f64 / f64::from(sr);
                audio = audio
                    .checked_add(
                        frame["nb_samples"]
                            .as_u64()
                            .context("audio frame samples missing")?,
                    )
                    .context("timestamp sample overflow")?;
                t
            }
            _ => bail!("unexpected master frame type"),
        };
        if !time.is_finite() || (time - expected).abs() > 0.002 {
            bail!("master timestamp gap/offset differs from continuous frame/sample timeline");
        }
    }
    if video == 0 || audio == 0 {
        bail!("master timestamp coverage missing");
    }
    Ok(())
}

pub fn check(contract_path: &Path, root: &Path) -> Result<Report> {
    let contract: Contract = serde_yaml::from_slice(&fs::read(contract_path)?)?;
    if contract.schema != "reel.episode-delivery.v0.1"
        || contract.id.trim().is_empty()
        || contract.scenes.is_empty()
    {
        bail!("invalid episode contract");
    }
    let master = scene_delivery::checked_file(root, &contract.master)?;
    let mut ids = BTreeSet::new();
    let mut children = Vec::new();
    for s in &contract.scenes {
        if !ids.insert(&s.id) {
            bail!("duplicate scene id");
        }
        children.push(load_scene(root, s)?);
    }
    let first = &children[0];
    let (width, height, sr, fn_, fd) = (
        first.0.width,
        first.0.height,
        first.1.plan.sample_rate,
        first.1.plan.fps_numerator,
        first.1.plan.fps_denominator,
    );
    let info = scene_delivery::probe(&master)?;
    let streams = info["streams"]
        .as_array()
        .context("master streams missing")?;
    let v = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .context("master video missing")?;
    let a = streams
        .iter()
        .find(|s| s["codec_type"] == "audio")
        .context("master audio missing")?;
    let (rate_n, rate_d) = v["r_frame_rate"]
        .as_str()
        .and_then(|s| s.split_once('/'))
        .context("master frame rate missing")?;
    let (rate_n, rate_d) = (rate_n.parse::<u64>()?, rate_d.parse::<u64>()?);
    if streams.len() != 2
        || v["codec_name"] != "ffv1"
        || v["pix_fmt"] != "yuv444p"
        || v["width"] != width
        || v["height"] != height
        || rate_d == 0
        || u128::from(rate_n) * u128::from(fd) != u128::from(rate_d) * u128::from(fn_)
        || a["codec_name"] != "pcm_s24le"
        || a["channels"] != 2
        || a["sample_rate"].as_str() != Some(&sr.to_string())
    {
        bail!("master must match scene FFV1/yuv444p and stereo PCM24 format");
    }
    let mut expected_layers = BTreeSet::new();
    let mut video_lengths = Vec::new();
    let mut audio_lengths = Vec::new();
    let mut consumptions = Vec::new();
    let mut frame_offset = 0u64;
    let mut sample_offset = 0u64;
    for (s, (job, r, dir)) in contract.scenes.iter().zip(&children) {
        verify_stem_sum(dir, r.content_samples)?;
        if (
            job.width,
            job.height,
            r.plan.sample_rate,
            r.plan.fps_numerator,
            r.plan.fps_denominator,
        ) != (width, height, sr, fn_, fd)
        {
            bail!("scene format mismatch");
        }
        let vl = r
            .plan
            .frame_count
            .checked_mul(u64::from(width) * u64::from(height) * 3)
            .context("video size overflow")?;
        let al = r
            .plan
            .duration_samples
            .checked_mul(6)
            .context("audio size overflow")?;
        video_lengths.push(vl);
        audio_lengths.push(al);
        let vh = decode_segments(&dir.join("picture.mkv"), true, &[vl])?.remove(0);
        let ah = decode_segments(&dir.join("mix.wav"), false, &[al])?.remove(0);
        // Also detect a resealed scene master with missing/replaced audio or picture.
        if decode_segments(&dir.join("master.mkv"), true, &[vl])?[0] != vh
            || decode_segments(&dir.join("master.mkv"), false, &[al])?[0] != ah
        {
            bail!("scene master differs from picture/mix");
        }
        consumptions.push(Consumption {
            scene_id: s.id.clone(),
            receipt_sha256: s.receipt.sha256.clone(),
            job_sha256: s.job.sha256.clone(),
            start_frame: frame_offset,
            start_sample: sample_offset,
            frames: r.plan.frame_count,
            samples: r.plan.duration_samples,
            picture_sha256: r.outputs["picture.mkv"].sha256.clone(),
            mix_sha256: r.outputs["mix.wav"].sha256.clone(),
            decoded_picture_sha256: vh,
            decoded_mix_sha256: ah,
        });
        frame_offset = frame_offset
            .checked_add(r.plan.frame_count)
            .context("frame overflow")?;
        sample_offset = sample_offset
            .checked_add(r.plan.duration_samples)
            .context("sample overflow")?;
        // Separate scene rounding may accumulate. Do not hide growing A/V drift.
        let drift = (u128::from(frame_offset) * u128::from(sr) * u128::from(fd))
            .abs_diff(u128::from(sample_offset) * u128::from(fn_));
        if drift > u128::from(sr) * u128::from(fd) {
            bail!("cumulative scene rounding exceeds one frame; reconform explicitly");
        }
        for ext in &job.external_layers {
            expected_layers.insert((s.id.clone(), ext.attachment_id.clone()));
        }
    }
    let mut declared_layers = BTreeSet::new();
    for layer in &contract.layers {
        if !declared_layers.insert((layer.scene_id.clone(), layer.attachment_id.clone()))
            || layer.reason.trim().is_empty()
        {
            bail!("duplicate/incomplete layer disposition");
        }
        let (job, _, _) = &children[contract
            .scenes
            .iter()
            .position(|s| s.id == layer.scene_id)
            .context("layer scene missing")?];
        let ext = job
            .external_layers
            .iter()
            .find(|e| e.attachment_id == layer.attachment_id)
            .context("unknown external layer")?;
        scene_delivery::checked_file(root, &layer.evidence)?;
        if layer.evidence.sha256 != ext.evidence.sha256
            || layer.evidence.bytes != ext.evidence.bytes
        {
            bail!("layer evidence differs from scene evidence");
        }
        match layer.disposition.as_str() {
            "baked-picture" => {
                let picture = job
                    .pictures
                    .iter()
                    .find(|p| Some(&p.attachment_id) == layer.picture_attachment_id.as_ref())
                    .context("baked layer picture missing")?;
                if picture.kind != scene_delivery::PictureKind::Video {
                    bail!("baked layer must bind a precomposed video");
                }
            }
            "excluded-from-clean-master" if layer.picture_attachment_id.is_none() => {}
            _ => bail!("unsupported layer disposition"),
        }
    }
    if expected_layers != declared_layers {
        bail!("every external layer needs an explicit disposition");
    }
    let got_v = decode_segments(&master, true, &video_lengths)?;
    let got_a = decode_segments(&master, false, &audio_lengths)?;
    for (i, c) in consumptions.iter().enumerate() {
        if got_v[i] != c.decoded_picture_sha256 || got_a[i] != c.decoded_mix_sha256 {
            bail!(
                "episode does not consume scene {} picture/mix at declared offset",
                c.scene_id
            );
        }
    }
    verify_timestamps(&master, sr, fn_, fd)?;
    let mut findings = Vec::new();
    for (i, pair) in children.windows(2).enumerate() {
        let (_, l, ld) = &pair[0];
        let (_, r, rd) = &pair[1];
        let lmix = edge_audio(&ld.join("mix.wav"), l.content_samples, sr, true)?;
        let rmix = edge_audio(&rd.join("mix.wav"), r.content_samples, sr, false)?;
        let jump = (0..2)
            .map(|ch| f64::from((lmix[lmix.len() - 2 + ch] - rmix[ch]).abs()))
            .fold(0.0, f64::max);
        let le = edge_audio(&ld.join("E.wav"), l.content_samples, sr, true)?;
        let re = edge_audio(&rd.join("E.wav"), r.content_samples, sr, false)?;
        let music = edge_audio(&ld.join("M.wav"), l.content_samples, sr, true)?;
        let tail = music[music.len() - 2..]
            .iter()
            .map(|v| f64::from(v.abs()))
            .fold(0.0, f64::max);
        let black = black_edge(
            &ld.join("picture.mkv"),
            l.delivery_frames - 1,
            width,
            height,
        )? || black_edge(&rd.join("picture.mkv"), 0, width, height)?;
        findings.extend(boundary_findings(
            &contract.scenes[i].id,
            &contract.scenes[i + 1].id,
            jump,
            (rms_db(&le) - rms_db(&re)).abs(),
            tail,
            black,
        ));
    }
    let mut decisions = BTreeSet::new();
    for d in &contract.boundary_decisions {
        let left = contract
            .scenes
            .iter()
            .find(|s| s.id == d.left_scene)
            .context("decision left scene missing")?;
        let right = contract
            .scenes
            .iter()
            .find(|s| s.id == d.right_scene)
            .context("decision right scene missing")?;
        if d.left_receipt_sha256 != left.receipt.sha256
            || d.right_receipt_sha256 != right.receipt.sha256
        {
            bail!("stale boundary decision scene receipts");
        }
        if d.owner.trim().is_empty()
            || d.reason.trim().is_empty()
            || !decisions.insert((&d.left_scene, &d.right_scene, &d.code))
        {
            bail!("invalid/duplicate boundary decision");
        }
        scene_delivery::checked_file(root, &d.evidence)?;
        let f = findings
            .iter_mut()
            .find(|f| {
                f.left_scene == d.left_scene && f.right_scene == d.right_scene && f.code == d.code
            })
            .context("stale or unmatched boundary decision")?;
        f.disposition = "documented-intentional-boundary".into();
    }
    Ok(Report {schema:"reel.episode-delivery-check.v0.1".into(),contract_sha256:crate::sha256_file(contract_path)?,master_sha256:contract.master.sha256,content_verified:true,passed:findings.iter().all(|f|f.disposition!="needs-review"),scenes:consumptions,boundary_findings:findings,layers:contract.layers,limitations:vec!["Exact decoded scene picture/mix inclusion; no creative or release approval.".into(),"External-layer evidence binds declared precompositions, not semantic VFX correctness or separate delivery tracks. Run their existing validators.".into(),"Boundary thresholds are review heuristics; equal-level ambience changes and all perceptual defects cannot be detected automatically.".into()]})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundary_metrics_preserve_distinct_findings() {
        let f = boundary_findings("a", "b", 0.2, 18.0, 0.03, true);
        assert_eq!(f.len(), 4);
        assert!(f.iter().all(|f| f.disposition == "needs-review"));
        assert!(boundary_findings("a", "b", 0.0, 0.0, 0.0, false).is_empty());
    }
}
