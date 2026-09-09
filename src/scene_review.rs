//! Controlled reviews for scene-delivery receipts; reuse the audio quality gate.
use crate::{
    audio_quality::{self, AudioCheckOptions, AudioProfile},
    episode_delivery::{self, Scene},
    scene_delivery::{self, FileRef},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub schema: String,
    pub baseline: Scene,
    pub candidate: Scene,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub contract_sha256: String,
    pub baseline_receipt_sha256: String,
    pub candidate_receipt_sha256: String,
    pub identical_dialogue_sha256: String,
    pub outputs: BTreeMap<String, FileRef>,
    pub limitations: String,
}

fn validate(contract_path: &Path, root: &Path) -> Result<(Contract, String)> {
    let c: Contract = serde_yaml::from_slice(&fs::read(contract_path)?)?;
    if c.schema != "reel.scene-review.v0.1" {
        bail!("invalid scene review contract");
    }
    let (aj, a, ad) = episode_delivery::load_scene(root, &c.baseline)?;
    let (bj, b, bd) = episode_delivery::load_scene(root, &c.candidate)?;
    if (
        aj.width,
        aj.height,
        a.plan.sample_rate,
        a.plan.duration_samples,
        a.plan.frame_count,
        a.plan.fps_numerator,
        a.plan.fps_denominator,
    ) != (
        bj.width,
        bj.height,
        b.plan.sample_rate,
        b.plan.duration_samples,
        b.plan.frame_count,
        b.plan.fps_numerator,
        b.plan.fps_denominator,
    ) {
        bail!("comparison requires identical timing and geometry");
    }
    let len = a
        .content_samples
        .checked_mul(6)
        .ok_or_else(|| anyhow::anyhow!("sample size overflow"))?;
    let ah = episode_delivery::decode_segments(&ad.join("D.wav"), false, &[len])?.remove(0);
    let bh = episode_delivery::decode_segments(&bd.join("D.wav"), false, &[len])?.remove(0);
    if ah != bh {
        bail!(
            "picture comparison requires identical decoded dialogue; route performance changes separately"
        );
    }
    Ok((c, ah))
}

pub fn render(contract_path: &Path, root: &Path, output: &Path) -> Result<Receipt> {
    let (contract, dialogue) = validate(contract_path, root)?;
    if output.exists() {
        bail!("review directory already exists");
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".scene-review-")
        .tempdir_in(parent)?;
    for (label, scene) in [("a", &contract.baseline), ("b", &contract.candidate)] {
        let (_, receipt, dir) = episode_delivery::load_scene(root, scene)?;
        let no_score = stage.path().join(format!("{label}-no-score.wav"));
        let em = stage.path().join(format!("{label}-effects-music.wav"));
        for (left, right, destination) in [("D.wav", "E.wav", &no_score), ("M.wav", "E.wav", &em)] {
            let float = stage.path().join(format!("{label}-float.wav"));
            scene_delivery::ffmpeg(&[
                "-i".into(),
                dir.join(left).to_string_lossy().into_owned(),
                "-i".into(),
                dir.join(right).to_string_lossy().into_owned(),
                "-filter_complex".into(),
                "[0:a][1:a]amix=inputs=2:normalize=0:duration=longest[a]".into(),
                "-map".into(),
                "[a]".into(),
                "-c:a".into(),
                "pcm_f32le".into(),
                float.to_string_lossy().into_owned(),
            ])?;
            scene_delivery::finish_pcm(&float, destination)?;
        }
        for (variant, audio) in [
            ("dialogue", dir.join("D.wav")),
            ("no-score", no_score),
            ("full", dir.join("mix.wav")),
        ] {
            let video = stage.path().join(format!("{label}-{variant}.mp4"));
            scene_delivery::ffmpeg(&[
                "-i".into(),
                dir.join("picture.mkv").to_string_lossy().into_owned(),
                "-i".into(),
                audio.to_string_lossy().into_owned(),
                "-map".into(),
                "0:v:0".into(),
                "-map".into(),
                "1:a:0".into(),
                "-c:v".into(),
                "libx264".into(),
                "-crf".into(),
                "18".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "192k".into(),
                "-movflags".into(),
                "+faststart".into(),
                video.to_string_lossy().into_owned(),
            ])?;
            // Inspect decode length without interpreting AAC padding as cue time.
            let o = std::process::Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-nostdin",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-i",
                ])
                .arg(&video)
                .args(["-f", "null", "-"])
                .output()?;
            if !o.status.success() {
                bail!("review derivative decode failed");
            }
        }
        let narration = dir.join("D.wav");
        let mix = dir.join("mix.wav");
        let quality = audio_quality::check_native(AudioCheckOptions {
            audio: &mix,
            narration_stem: Some(&narration),
            effects_music_stem: Some(&em),
            manifest: None,
            profile: AudioProfile::PrivateReview,
        })?;
        fs::write(
            stage.path().join(format!("{label}-audio-check.json")),
            serde_json::to_vec_pretty(&quality)?,
        )?;
        fs::write(
            stage.path().join(format!("{label}-scene-receipt.json")),
            serde_json::to_vec_pretty(&receipt)?,
        )?;
    }
    fs::write(
        stage.path().join("index.html"),
        r#"<!doctype html><meta charset="utf-8"><title>Scene comparison</title>
<style>body{font:18px system-ui;background:#171717;color:#eee;margin:2rem}video{width:min(100%,800px)}select,button{font:inherit;margin:.5rem}a{color:#aaf}</style>
<h1>Scene comparison</h1><p>A is baseline; B is candidate. Dialogue PCM and timing are identical. Switch at the same playhead. Labels stay outside the picture.</p>
<label>Version<select id="version"><option value="a">A</option><option value="b">B</option></select></label>
<label>Sound<select id="sound"><option value="dialogue">Dialogue only</option><option value="no-score">Dialogue + effects</option><option value="full">Full mix</option></select></label><br>
<video id="player" controls src="a-dialogue.mp4"></video>
<p>First compare picture on dialogue only; then compare environmental sound; finally hear the score. Effects-on/off requires supplying those exact A/B scene deliveries.</p>
<p><a href="a-audio-check.json">A audio measurements</a> / <a href="b-audio-check.json">B audio measurements</a>. Failed audio checks remain findings. This review is not approval.</p>
<script>const p=document.getElementById('player');function swap(){const t=p.currentTime;const playing=!p.paused;p.src=document.getElementById('version').value+'-'+document.getElementById('sound').value+'.mp4';p.addEventListener('loadedmetadata',()=>{p.currentTime=Math.min(t,p.duration);if(playing)p.play();},{once:true});}document.querySelectorAll('select').forEach(s=>s.onchange=swap);</script>"#,
    )?;
    let mut outputs = BTreeMap::new();
    for entry in fs::read_dir(stage.path())? {
        let p = entry?.path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        outputs.insert(
            name.clone(),
            FileRef {
                path: name.into(),
                sha256: crate::sha256_file(&p)?,
                bytes: fs::metadata(p)?.len(),
            },
        );
    }
    let receipt=Receipt {schema:"reel.scene-review-receipt.v0.1".into(),contract_sha256:crate::sha256_file(contract_path)?,baseline_receipt_sha256:contract.baseline.receipt.sha256,candidate_receipt_sha256:contract.candidate.receipt.sha256,identical_dialogue_sha256:dialogue,outputs,limitations:"A/B picture control fixes dialogue and timing; full/no-score variants intentionally change sound. Browser playback and audio measurements require human interpretation. No publication or selection granted.".into()};
    fs::write(
        stage.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    check(contract_path, root, stage.path())?;
    fs::rename(stage.path(), output)?;
    Ok(receipt)
}

pub fn check(contract_path: &Path, root: &Path, output: &Path) -> Result<Receipt> {
    let (c, dialogue) = validate(contract_path, root)?;
    let r: Receipt = serde_json::from_slice(&fs::read(output.join("receipt.json"))?)?;
    if r.schema != "reel.scene-review-receipt.v0.1"
        || r.contract_sha256 != crate::sha256_file(contract_path)?
        || r.baseline_receipt_sha256 != c.baseline.receipt.sha256
        || r.candidate_receipt_sha256 != c.candidate.receipt.sha256
        || r.identical_dialogue_sha256 != dialogue
    {
        bail!("stale scene review receipt");
    }
    let mut names = std::collections::BTreeSet::from(["index.html".to_string()]);
    for label in ["a", "b"] {
        for suffix in [
            "dialogue.mp4",
            "no-score.mp4",
            "full.mp4",
            "no-score.wav",
            "effects-music.wav",
            "audio-check.json",
            "scene-receipt.json",
        ] {
            names.insert(format!("{label}-{suffix}"));
        }
    }
    if r.outputs
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        != names
    {
        bail!("incomplete scene review output set");
    }
    for (name, item) in &r.outputs {
        if item.path != Path::new(name) {
            bail!("review output name mismatch");
        }
        scene_delivery::checked_file(output, item)?;
    }
    Ok(r)
}
