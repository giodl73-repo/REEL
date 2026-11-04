//! Bounded playback of calibrated float32 samples on an immutable native score clock.
//! No source separation, pitch inference, renderer invocation or creative approval.
use anyhow::{Result, bail};
use reel_music::hash::sha256_bytes as hash;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_AUDIO: usize = 128 * 1024 * 1024;
const MAX_SAMPLES: usize = 32 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Input {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}
impl Input {
    fn read(&self, max: usize) -> Result<Vec<u8>> {
        if !self.path.is_absolute() || self.bytes > max as u64 {
            bail!("input path or byte budget");
        }
        let mut raw = Vec::new();
        fs::File::open(&self.path)?
            .take(self.bytes + 1)
            .read_to_end(&mut raw)?;
        if raw.len() as u64 != self.bytes || hash(&raw) != self.sha256 {
            bail!("input identity differs");
        }
        Ok(raw)
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    bank_audio: Input,
    calibration: Input,
    native_score_midi: Input,
    sample_rate: u32,
    channels: u16,
    frames: u64,
    release_frames: u32,
    gain: f32,
    peak_ceiling: f32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    pitch: u8,
    audio_attack_frame: u64,
    held_sample_frames: u64,
    #[serde(default)]
    renderer_start_seconds: Option<f64>,
}
fn calibration(raw: &[u8]) -> Result<Vec<Sample>> {
    crate::approval_attestation::ensure_no_duplicate_keys(raw)?;
    let v: Value = serde_json::from_slice(raw)?;
    if !matches!(
        v["schema"].as_str(),
        Some("reel.piano-sample-clock-evidence.v1" | "reel.music-sample-bank.v1")
    ) {
        bail!("unsupported calibration contract");
    }
    let bank: Vec<Sample> = serde_json::from_value(v["bank"].clone())?;
    if bank.is_empty() || bank.len() > 128 {
        bail!("calibration pitch budget");
    }
    let mut pitches = BTreeSet::new();
    for s in &bank {
        if s.pitch > 127
            || !pitches.insert(s.pitch)
            || s.held_sample_frames == 0
            || s.renderer_start_seconds
                .is_some_and(|s| !s.is_finite() || s < 0.0)
        {
            bail!("invalid calibration sample");
        }
    }
    Ok(bank)
}

/// The initial input standard is ordinary IEEE float32 WAVE, mono or stereo.
fn decode_wave(raw: &[u8], rate: u32, channels: u16) -> Result<Vec<f32>> {
    if raw.len() < 12
        || &raw[..4] != b"RIFF"
        || &raw[8..12] != b"WAVE"
        || u32::from_le_bytes(raw[4..8].try_into()?) as usize + 8 != raw.len()
    {
        bail!("invalid sample-bank WAVE container");
    }
    let mut offset = 12usize;
    let mut format_seen = false;
    let mut data = None;
    let mut chunks = 0;
    while offset < raw.len() {
        chunks += 1;
        if chunks > 512 || offset + 8 > raw.len() {
            bail!("WAVE chunk budget");
        }
        let size = u32::from_le_bytes(raw[offset + 4..offset + 8].try_into()?) as usize;
        let end = offset
            .checked_add(8)
            .and_then(|n| n.checked_add(size))
            .ok_or_else(|| anyhow::anyhow!("WAVE chunk overflow"))?;
        let chunk = raw
            .get(offset + 8..end)
            .ok_or_else(|| anyhow::anyhow!("truncated WAVE chunk"))?;
        match &raw[offset..offset + 4] {
            b"fmt " => {
                if format_seen
                    || chunk.len() < 16
                    || u16::from_le_bytes(chunk[0..2].try_into()?) != 3
                    || u16::from_le_bytes(chunk[2..4].try_into()?) != channels
                    || u32::from_le_bytes(chunk[4..8].try_into()?) != rate
                    || u32::from_le_bytes(chunk[8..12].try_into()?) != rate * channels as u32 * 4
                    || u16::from_le_bytes(chunk[12..14].try_into()?) != channels * 4
                    || u16::from_le_bytes(chunk[14..16].try_into()?) != 32
                {
                    bail!("sample-bank format differs from declared float32 clock");
                }
                format_seen = true;
            }
            b"data" if data.is_some() => bail!("duplicate WAVE data"),
            b"data" => data = Some(chunk),
            _ => {}
        }
        offset = end
            .checked_add(size % 2)
            .ok_or_else(|| anyhow::anyhow!("WAVE padding overflow"))?;
    }
    let data = data.ok_or_else(|| anyhow::anyhow!("WAVE data missing"))?;
    if !format_seen
        || offset != raw.len()
        || data.is_empty()
        || data.len() % (channels as usize * 4) != 0
    {
        bail!("invalid sample-bank data");
    }
    data.chunks_exact(4)
        .map(|b| {
            let value = f32::from_le_bytes(b.try_into().unwrap());
            if !value.is_finite() {
                bail!("non-finite sample-bank audio");
            }
            Ok(value)
        })
        .collect()
}

fn encode_pcm24(samples: &[f32], rate: u32, channels: u16, scale: f32) -> Result<Vec<u8>> {
    let size = u32::try_from(
        samples
            .len()
            .checked_mul(3)
            .ok_or_else(|| anyhow::anyhow!("output size"))?,
    )?;
    let mut out = Vec::with_capacity(size as usize + 44);
    out.extend(b"RIFF");
    out.extend((size + 36).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(channels.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * channels as u32 * 3).to_le_bytes());
    out.extend((channels * 3).to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend(b"data");
    out.extend(size.to_le_bytes());
    for &value in samples {
        let q = (value as f64 * scale as f64 * 8_388_608.0).round();
        if !q.is_finite() || !(-8_388_608.0..=8_388_607.0).contains(&q) {
            bail!("PCM24 overload");
        }
        out.extend(&(q as i32).to_le_bytes()[..3]);
    }
    Ok(out)
}

/// Writes one new private result directory after validating and capturing every input.
pub fn render(request_path: &Path, output: &Path) -> Result<Value> {
    if !output.is_absolute() || output.exists() {
        bail!("output must be a new absolute directory");
    }
    let mut raw = Vec::new();
    fs::File::open(request_path)?
        .take(65_537)
        .read_to_end(&mut raw)?;
    if raw.len() > 65_536 {
        bail!("request budget");
    }
    crate::approval_attestation::ensure_no_duplicate_keys(&raw)?;
    let r: Request = serde_json::from_slice(&raw)?;
    if r.schema != "reel.music-sample-clock-render.v1"
        || !(8000..=192000).contains(&r.sample_rate)
        || !matches!(r.channels, 1 | 2)
        || r.frames == 0
        || r.release_frames > r.sample_rate
        || !r.gain.is_finite()
        || !(0.0..=16.0).contains(&r.gain)
        || !r.peak_ceiling.is_finite()
        || !(0.0..1.0).contains(&r.peak_ceiling)
        || r.peak_ceiling == 0.0
    {
        bail!("sample-clock request contract");
    }
    let count = usize::try_from(r.frames)?
        .checked_mul(r.channels as usize)
        .filter(|n| *n <= MAX_SAMPLES)
        .ok_or_else(|| anyhow::anyhow!("output sample budget"))?;
    let calibration_raw = r.calibration.read(1024 * 1024)?;
    let windows = calibration(&calibration_raw)?;
    let midi = r.native_score_midi.read(1024 * 1024)?;
    let notes = crate::music_score::native_note_clock(&midi, r.sample_rate)?;
    let mut by_pitch = BTreeMap::new();
    for window in windows {
        by_pitch.insert(window.pitch, window);
    }
    let mut scheduled = Vec::new();
    let mut contributions = 0u64;
    for n in &notes {
        let window = by_pitch
            .get(&n.pitch)
            .ok_or_else(|| anyhow::anyhow!("required pitch missing"))?;
        if n.start >= n.release || n.release > r.frames {
            bail!("note outside output clock");
        }
        let held = n.release - n.start;
        let length = held
            .checked_add(r.release_frames as u64)
            .ok_or_else(|| anyhow::anyhow!("release overflow"))?
            .min(r.frames - n.start);
        if window.held_sample_frames < length {
            bail!("calibration hold too short");
        }
        contributions = contributions
            .checked_add(length * r.channels as u64)
            .filter(|v| *v <= 512 * 1024 * 1024)
            .ok_or_else(|| anyhow::anyhow!("mix work budget"))?;
        scheduled.push((n, window, held, length));
    }
    // Refuse invalid controls before opening the larger primary audio.
    let source = r.bank_audio.read(MAX_AUDIO)?;
    let bank = decode_wave(&source, r.sample_rate, r.channels)?;
    for window in by_pitch.values() {
        let end = window
            .audio_attack_frame
            .checked_add(window.held_sample_frames)
            .ok_or_else(|| anyhow::anyhow!("sample window overflow"))?;
        if end > (bank.len() / r.channels as usize) as u64 {
            bail!("sample window outside bank");
        }
        let start_sample = window.audio_attack_frame as usize * r.channels as usize;
        let end_sample = end as usize * r.channels as usize;
        if !bank[start_sample..end_sample]
            .iter()
            .any(|s| s.abs() >= 1e-5)
        {
            bail!("inaudible calibration sample");
        }
    }
    let mut pcm = vec![0f32; count];
    for (n, window, held, length) in scheduled {
        let velocity = n.velocity as f64 / 64.0;
        let amplitude = (velocity * velocity.sqrt()) as f32 * r.gain;
        for frame in 0..length {
            let tail = length - held;
            let envelope = if frame < held || tail <= 1 {
                1.0
            } else {
                let position = (frame - held) as f32 / (tail - 1) as f32;
                (1.0 - position) * (1.0 - position)
            };
            for channel in 0..r.channels as usize {
                let from =
                    (window.audio_attack_frame + frame) as usize * r.channels as usize + channel;
                let to = (n.start + frame) as usize * r.channels as usize + channel;
                pcm[to] += bank[from] * amplitude * envelope;
            }
        }
    }
    let peak = pcm.iter().try_fold(0f32, |peak, s| {
        if !s.is_finite() {
            bail!("non-finite mix");
        }
        Ok(peak.max(s.abs()))
    })?;
    if peak <= 0.0 {
        bail!("silent sample-clock result");
    }
    let scale = (r.peak_ceiling / peak).min(1.0);
    let wav = encode_pcm24(&pcm, r.sample_rate, r.channels, scale)?;
    let receipt = json!({"schema":"reel.music-sample-clock-receipt.v1","request_sha256":hash(&raw),
        "bank_audio":r.bank_audio,"calibration":r.calibration,"native_score_midi":r.native_score_midi,
        "sample_rate":r.sample_rate,"channels":r.channels,"frames":r.frames,"notes":notes.len(),
        "release_frames":r.release_frames,"gain":r.gain,"attenuation":scale,"output_peak":peak*scale,
        "velocity_policy":"(velocity/64)^1.5","clock_policy":"native ticks; nearest frame, half upward",
        "schedule":notes.iter().map(|n| json!({"part_id":n.part_id,"voice":n.voice,"pitch":n.pitch,"velocity":n.velocity,
            "start_frame":n.start,"key_release_frame":n.release,"release_tail_frames":(r.frames-n.release).min(r.release_frames as u64)})).collect::<Vec<_>>(),
        "output":{"sha256":hash(&wav),"bytes":wav.len()},"human_approval":false,"published":false});
    fs::create_dir(output)?;
    for (name, bytes) in [
        ("instrumental.wav", wav),
        ("receipt.json", serde_json::to_vec(&receipt)?),
    ] {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    Ok(
        json!({"notes":notes.len(),"frames":r.frames,"channels":r.channels,
        "sample_rate":r.sample_rate,"human_approval":false,"published":false,"git_changed":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(root: &Path, name: &str, raw: &[u8]) -> Input {
        let path = root.join(name);
        fs::write(&path, raw).unwrap();
        Input {
            path,
            sha256: hash(raw),
            bytes: raw.len() as u64,
        }
    }
    fn wave() -> Vec<u8> {
        let data: Vec<u8> = (0..3200)
            .flat_map(|_| [0.25f32, -0.125f32])
            .flat_map(f32::to_le_bytes)
            .collect();
        let mut out = b"RIFF".to_vec();
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(3u16.to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(8000u32.to_le_bytes());
        out.extend(64000u32.to_le_bytes());
        out.extend(8u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }
    fn midi(pitch: u8, doubled: bool, part_tempo: bool) -> Vec<u8> {
        let conductor = vec![0, 0xff, 0x51, 3, 0x0f, 0x42, 0x40, 40, 0xff, 0x2f, 0];
        let marker = b"REEL_PART:[\"piano\",\"Piano\"]";
        let mut part = vec![0, 0xff, 3, marker.len() as u8];
        part.extend(marker);
        if part_tempo {
            part.extend([0, 0xff, 0x51, 3, 7, 0xa1, 0x20]);
        }
        part.extend([5, 0x90, pitch, 64]);
        if doubled {
            part.extend([0, 0x91, pitch, 64]);
        }
        part.extend([20, 0x80, pitch, 0]);
        if doubled {
            part.extend([0, 0x81, pitch, 0]);
        }
        part.extend([15, 0xff, 0x2f, 0]);
        let mut raw = b"MThd".to_vec();
        raw.extend(6u32.to_be_bytes());
        raw.extend(1u16.to_be_bytes());
        raw.extend(2u16.to_be_bytes());
        raw.extend(100u16.to_be_bytes());
        for track in [conductor, part] {
            raw.extend(b"MTrk");
            raw.extend((track.len() as u32).to_be_bytes());
            raw.extend(track);
        }
        raw
    }
    fn request(root: &Path) -> Request {
        Request {schema:"reel.music-sample-clock-render.v1".into(),
            bank_audio:input(root,"bank.wav",&wave()),
            calibration:input(root,"bank.json",&serde_json::to_vec(&json!({"schema":"reel.music-sample-bank.v1","bank":[{"pitch":60,"audio_attack_frame":0,"held_sample_frames":3000}]})).unwrap()),
            native_score_midi:input(root,"score.mid",&midi(60,false,false)),
            sample_rate:8000,channels:2,frames:3200,release_frames:400,gain:1.0,peak_ceiling:0.89}
    }
    fn run(root: &Path, r: &Request, name: &str) -> Result<Value> {
        fs::write(root.join("request.json"), serde_json::to_vec(r)?).unwrap();
        render(&root.join("request.json"), &root.join(name))
    }
    fn pcm(raw: &[u8], frame: usize, channel: usize) -> i32 {
        let p = 44 + (frame * 2 + channel) * 3;
        let value = raw[p] as i32 | (raw[p + 1] as i32) << 8 | (raw[p + 2] as i32) << 16;
        (value << 8) >> 8
    }
    #[test]
    fn native_clock_preserves_stereo_onsets_release_and_source_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let r = request(root);
        let before = fs::read(&r.bank_audio.path).unwrap();
        let summary = run(root, &r, "render").unwrap();
        assert_eq!(summary["notes"], 1);
        assert_eq!(summary["published"], false);
        assert_eq!(summary["human_approval"], false);
        let raw = fs::read(root.join("render/instrumental.wav")).unwrap();
        assert_eq!(raw.len(), 44 + 3200 * 2 * 3);
        assert_eq!(pcm(&raw, 399, 0), 0);
        assert_eq!(pcm(&raw, 400, 0), 2_097_152);
        assert_eq!(pcm(&raw, 400, 1), -1_048_576);
        assert_eq!(pcm(&raw, 1999, 0), 2_097_152);
        assert_eq!(pcm(&raw, 2000, 0), 2_097_152);
        assert!(pcm(&raw, 2200, 0) > 0 && pcm(&raw, 2200, 0) < 600_000);
        assert_eq!(pcm(&raw, 2399, 0), 0);
        assert_eq!(pcm(&raw, 2400, 0), 0);
        assert_eq!(fs::read(&r.bank_audio.path).unwrap(), before);
        assert!(run(root, &r, "render").is_err());
        assert_eq!(fs::read(root.join("render/instrumental.wav")).unwrap(), raw);
    }
    #[test]
    fn overlapping_unisons_keep_both_voices_and_attenuate_without_clipping() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut r = request(root);
        r.native_score_midi = input(root, "double.mid", &midi(60, true, false));
        r.peak_ceiling = 0.25;
        assert_eq!(run(root, &r, "double").unwrap()["notes"], 2);
        let receipt: Value =
            serde_json::from_slice(&fs::read(root.join("double/receipt.json")).unwrap()).unwrap();
        assert_eq!(receipt["attenuation"], 0.5);
        let raw = fs::read(root.join("double/instrumental.wav")).unwrap();
        assert_eq!(pcm(&raw, 400, 0), 2_097_152);
    }
    #[test]
    fn mismatched_missing_short_external_or_invalid_sources_refuse_before_output() {
        for case in [
            "pin",
            "pitch",
            "short",
            "clock",
            "part-tempo",
            "nan",
            "budget",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            let mut r = request(root);
            match case {
                "pin"=>r.bank_audio.sha256="0".repeat(64),
                "pitch"=>r.native_score_midi=input(root,"missing.mid",&midi(61,false,false)),
                "short"=>r.calibration=input(root,"short.json",&serde_json::to_vec(&json!({"schema":"reel.music-sample-bank.v1","bank":[{"pitch":60,"audio_attack_frame":0,"held_sample_frames":10}]})).unwrap()),
                "clock"=>r.frames=1700,
                "part-tempo"=>r.native_score_midi=input(root,"external.mid",&midi(60,false,true)),
                "nan"=>{let mut raw=wave();raw[44..48].copy_from_slice(&f32::NAN.to_le_bytes());r.bank_audio=input(root,"nan.wav",&raw);},
                "budget"=>r.frames=u64::MAX,
                _=>unreachable!()
            }
            assert!(run(root, &r, "refused").is_err(), "{case}");
            assert!(!root.join("refused").exists());
        }
    }
}
