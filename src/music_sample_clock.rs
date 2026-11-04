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
    vocal: Option<Vocal>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Vocal {
    source: Input,
    gain: f32,
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

fn sum_pcm24(left: &[u8], right: &[u8]) -> Result<Vec<u8>> {
    if left.len() != right.len() || left.len() < 44 || left[..44] != right[..44] {
        bail!("stem PCM24 geometry differs");
    }
    let decode = |b: &[u8]| ((b[0] as i32 | (b[1] as i32) << 8 | (b[2] as i32) << 16) << 8) >> 8;
    let mut out = left[..44].to_vec();
    for (a, b) in left[44..].chunks_exact(3).zip(right[44..].chunks_exact(3)) {
        let q = decode(a) + decode(b);
        if !(-8_388_608..=8_388_607).contains(&q) {
            bail!("stem sum overload");
        }
        out.extend(&q.to_le_bytes()[..3]);
    }
    Ok(out)
}

/// Writes one new private result directory after validating and capturing every input.
pub fn render(request_path: &Path, output: &Path) -> Result<Value> {
    render_bytes(&read_bounded(request_path, 65_536)?, output)
}

pub(crate) fn render_bytes(raw: &[u8], output: &Path) -> Result<Value> {
    if !output.is_absolute() || output.exists() {
        bail!("output must be a new absolute directory");
    }
    let r = parse_request(raw)?;
    let count = usize::try_from(r.frames)?
        .checked_mul(r.channels as usize)
        .filter(|n| *n <= MAX_SAMPLES)
        .ok_or_else(|| anyhow::anyhow!("output sample budget"))?;
    render_request(raw, r, output, count)
}

fn parse_request(raw: &[u8]) -> Result<Request> {
    if raw.len() > 65_536 {
        bail!("request budget");
    }
    crate::approval_attestation::ensure_no_duplicate_keys(raw)?;
    let r: Request = serde_json::from_slice(raw)?;
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
        || r.vocal
            .as_ref()
            .is_some_and(|v| !v.gain.is_finite() || !(0.0..=16.0).contains(&v.gain))
    {
        bail!("sample-clock request contract");
    }
    usize::try_from(r.frames)?
        .checked_mul(r.channels as usize)
        .filter(|n| *n <= MAX_SAMPLES)
        .ok_or_else(|| anyhow::anyhow!("output sample budget"))?;
    if r.bank_audio.bytes > MAX_AUDIO as u64
        || r.calibration.bytes > 1024 * 1024
        || r.native_score_midi.bytes > 1024 * 1024
        || r.vocal
            .as_ref()
            .is_some_and(|v| v.source.bytes > MAX_AUDIO as u64)
    {
        bail!("input byte budget");
    }
    Ok(r)
}

/// Validate controls and estimate bounded PCM outputs before provider media IO.
#[cfg(feature = "tig")]
pub(crate) fn output_byte_bound(raw: &[u8]) -> Result<u64> {
    let r = parse_request(raw)?;
    // Three metadata packets plus the maximum bounded note receipt.
    Ok(
        (44 + r.frames * r.channels as u64 * 3) * if r.vocal.is_some() { 3 } else { 1 }
            + 4 * 1024 * 1024,
    )
}

fn render_request(raw: &[u8], r: Request, output: &Path, count: usize) -> Result<Value> {
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
    drop(bank);
    drop(source);
    let vocal_pcm = if let Some(v) = &r.vocal {
        let raw = v.source.read(MAX_AUDIO)?;
        let mut decoded = decode_wave(&raw, r.sample_rate, r.channels)?;
        if decoded.len() != count {
            bail!("vocal frame count differs; no implicit trim, padding or resampling");
        }
        for s in &mut decoded {
            *s *= v.gain;
        }
        Some(decoded)
    } else {
        None
    };
    let mut peak = pcm.iter().try_fold(0f32, |peak, s| {
        if !s.is_finite() {
            bail!("non-finite mix");
        }
        Ok(peak.max(s.abs()))
    })?;
    if let Some(vocal) = &vocal_pcm {
        for (&p, &v) in pcm.iter().zip(vocal) {
            if !v.is_finite() || !(p + v).is_finite() {
                bail!("non-finite vocal mix");
            }
            peak = peak.max(v.abs()).max((p + v).abs());
        }
    }
    if peak <= 0.0 {
        bail!("silent sample-clock result");
    }
    // Leave rounding headroom when independently quantizing two stems.
    let ceiling = r.peak_ceiling
        - if r.vocal.is_some() {
            3.0 / 8_388_608.0
        } else {
            0.0
        };
    if ceiling <= 0.0 {
        bail!("peak ceiling below PCM24 stem-sum headroom");
    }
    let scale = (ceiling / peak).min(1.0);
    let wav = encode_pcm24(&pcm, r.sample_rate, r.channels, scale)?;
    let mut files = vec![("instrumental.wav", wav)];
    if let Some(vocal) = &vocal_pcm {
        files.push((
            "vocal-stem.wav",
            encode_pcm24(vocal, r.sample_rate, r.channels, scale)?,
        ));
    }
    drop(pcm);
    drop(vocal_pcm);
    if r.vocal.is_some() {
        let master = sum_pcm24(&files[0].1, &files[1].1)?;
        files.push(("with-vocals.wav", master));
    }
    if files.last().unwrap().1[44..].iter().all(|b| *b == 0) {
        bail!("quantized sample-clock result is silent");
    }
    let outputs: BTreeMap<_, _> = files
        .iter()
        .map(|(name, bytes)| (*name, json!({"sha256":hash(bytes),"bytes":bytes.len()})))
        .collect();
    let receipt = json!({"schema":"reel.music-sample-clock-receipt.v1","request_sha256":hash(raw),
        "bank_audio":r.bank_audio,"calibration":r.calibration,"native_score_midi":r.native_score_midi,
        "sample_rate":r.sample_rate,"channels":r.channels,"frames":r.frames,"notes":notes.len(),
        "release_frames":r.release_frames,"gain":r.gain,"attenuation":scale,"output_peak":peak*scale,
        "velocity_policy":"(velocity/64)^1.5","clock_policy":"native ticks; nearest frame, half upward",
        "schedule":notes.iter().map(|n| json!({"part_id":n.part_id,"voice":n.voice,"pitch":n.pitch,"velocity":n.velocity,
            "start_frame":n.start,"key_release_frame":n.release,"release_tail_frames":(r.frames-n.release).min(r.release_frames as u64)})).collect::<Vec<_>>(),
        "output":outputs["instrumental.wav"],"outputs":outputs,"vocal":r.vocal,
        "vocal_retimed":false,"vocal_regenerated":false,"vocal_pitched":false,
        "stem_sum_policy":"master is exact sum of quantized PCM24 stems",
        "human_approval":false,"published":false});
    fs::create_dir(output)?;
    files.push(("receipt.json", serde_json::to_vec(&receipt)?));
    for (name, bytes) in files {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    Ok(
        json!({"notes":notes.len(),"frames":r.frames,"channels":r.channels,
        "sample_rate":r.sample_rate,"vocal_mixed":r.vocal.is_some(),"human_approval":false,"published":false,"git_changed":false}),
    )
}

fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > max {
        bail!("check input byte budget");
    }
    Ok(raw)
}
fn pcm_geometry(raw: &[u8], r: &Request, count: usize) -> Result<()> {
    if raw.len() != 44 + count * 3
        || &raw[..4] != b"RIFF"
        || &raw[8..16] != b"WAVEfmt "
        || u32::from_le_bytes(raw[4..8].try_into()?) as usize + 8 != raw.len()
        || u32::from_le_bytes(raw[16..20].try_into()?) != 16
        || u16::from_le_bytes(raw[20..22].try_into()?) != 1
        || u16::from_le_bytes(raw[22..24].try_into()?) != r.channels
        || u32::from_le_bytes(raw[24..28].try_into()?) != r.sample_rate
        || u32::from_le_bytes(raw[28..32].try_into()?) != r.sample_rate * r.channels as u32 * 3
        || u16::from_le_bytes(raw[32..34].try_into()?) != r.channels * 3
        || u16::from_le_bytes(raw[34..36].try_into()?) != 24
        || &raw[36..40] != b"data"
        || u32::from_le_bytes(raw[40..44].try_into()?) as usize != count * 3
    {
        bail!("rendered PCM24 clock or geometry differs");
    }
    Ok(())
}
fn signed24(raw: &[u8]) -> i32 {
    ((raw[0] as i32 | (raw[1] as i32) << 8 | (raw[2] as i32) << 16) << 8) >> 8
}

/// Read-only packet checking. No rerender, source mutation or approval inference.
pub fn check(request_path: &Path, output: &Path) -> Result<Value> {
    let raw = read_bounded(request_path, 65_536)?;
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
        || r.vocal
            .as_ref()
            .is_some_and(|v| !v.gain.is_finite() || !(0.0..=16.0).contains(&v.gain))
    {
        bail!("invalid check request");
    }
    let count = usize::try_from(r.frames)?
        .checked_mul(r.channels as usize)
        .filter(|n| *n <= MAX_SAMPLES)
        .ok_or_else(|| anyhow::anyhow!("check sample budget"))?;
    let receipt_raw = read_bounded(&output.join("receipt.json"), 16 * 1024 * 1024)?;
    crate::approval_attestation::ensure_no_duplicate_keys(&receipt_raw)?;
    let receipt: Value = serde_json::from_slice(&receipt_raw)?;
    if receipt["schema"] != "reel.music-sample-clock-receipt.v1"
        || receipt["request_sha256"] != hash(&raw)
        || receipt["frames"] != r.frames
        || receipt["channels"] != r.channels
        || receipt["sample_rate"] != r.sample_rate
        || receipt["human_approval"] != false
        || receipt["published"] != false
    {
        bail!("receipt context differs");
    }
    for (name, input, max) in [
        ("bank_audio", &r.bank_audio, MAX_AUDIO),
        ("calibration", &r.calibration, 1024 * 1024),
        ("native_score_midi", &r.native_score_midi, 1024 * 1024),
    ] {
        if receipt[name] != serde_json::to_value(input)? {
            bail!("receipt input binding differs");
        }
        input.read(max)?;
    }
    let notes = crate::music_score::native_note_clock(
        &r.native_score_midi.read(1024 * 1024)?,
        r.sample_rate,
    )?;
    if receipt["notes"] != notes.len()
        || receipt["gain"] != r.gain
        || receipt["release_frames"] != r.release_frames
    {
        bail!("receipt note count differs");
    }
    let schedule = receipt["schedule"]
        .as_array()
        .filter(|rows| rows.len() == notes.len())
        .ok_or_else(|| anyhow::anyhow!("receipt schedule missing"))?;
    for (n, s) in notes.iter().zip(schedule) {
        if n.start >= n.release
            || n.release > r.frames
            || s["part_id"] != n.part_id
            || s["voice"] != n.voice
            || s["pitch"] != n.pitch
            || s["velocity"] != n.velocity
            || s["start_frame"] != n.start
            || s["key_release_frame"] != n.release
            || s["release_tail_frames"] != (r.frames - n.release).min(r.release_frames as u64)
        {
            bail!("receipt schedule differs from native source clock");
        }
    }
    let scale = receipt["attenuation"]
        .as_f64()
        .filter(|s| s.is_finite() && *s > 0.0 && *s <= 1.0)
        .ok_or_else(|| anyhow::anyhow!("invalid attenuation"))? as f32;
    let mut names = vec!["instrumental.wav"];
    if r.vocal.is_some() {
        names.extend(["vocal-stem.wav", "with-vocals.wav"]);
    }
    let mut outputs = BTreeMap::new();
    for name in names {
        let bytes = read_bounded(&output.join(name), MAX_SAMPLES * 3 + 44)?;
        pcm_geometry(&bytes, &r, count)?;
        let pin = if name == "instrumental.wav" && receipt["outputs"].is_null() {
            &receipt["output"]
        } else {
            &receipt["outputs"][name]
        };
        if pin["sha256"] != hash(&bytes) || pin["bytes"] != bytes.len() {
            bail!("output identity differs");
        }
        outputs.insert(name, bytes);
    }
    if let Some(v) = &r.vocal {
        if !v.gain.is_finite()
            || !(0.0..=16.0).contains(&v.gain)
            || receipt["vocal"] != serde_json::to_value(v)?
            || receipt["vocal_retimed"] != false
            || receipt["vocal_regenerated"] != false
            || receipt["vocal_pitched"] != false
        {
            bail!("vocal source context differs");
        }
        let source_raw = v.source.read(MAX_AUDIO)?;
        let source = decode_wave(&source_raw, r.sample_rate, r.channels)?;
        if source.len() != count {
            bail!("vocal source frame count differs");
        }
        let p = &outputs["instrumental.wav"][44..];
        let s = &outputs["vocal-stem.wav"][44..];
        let m = &outputs["with-vocals.wav"][44..];
        for (i, ((p, s), m)) in p
            .chunks_exact(3)
            .zip(s.chunks_exact(3))
            .zip(m.chunks_exact(3))
            .enumerate()
        {
            if signed24(p) + signed24(s) != signed24(m) {
                bail!("master is not the exact PCM24 stem sum");
            }
            let expected = ((source[i] * v.gain) as f64 * scale as f64 * 8_388_608.0).round();
            if expected != signed24(s) as f64 {
                bail!("vocal PCM differs from the unchanged source and declared gain");
            }
        }
    }
    Ok(
        json!({"outputs_verified":outputs.len(),"frames":r.frames,"source_pins_verified":true,
        "vocal_pcm_verified":r.vocal.is_some(),"exact_stem_sum_verified":r.vocal.is_some(),
        "complete_semantic_coverage":false,"human_approval":false,"published":false,"git_changed":false}),
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
            sample_rate:8000,channels:2,frames:3200,release_frames:400,gain:1.0,peak_ceiling:0.89,vocal:None}
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
    #[cfg(all(windows, feature = "tig"))]
    #[test]
    fn tig_source_render_preserves_pcm_and_refuses_stale_pins_and_unsafe_outputs() {
        use std::process::Command;
        use tig_core::Object;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let r = request(root);
        run(root, &r, "baseline").unwrap();
        let repo = root.join("repo");
        let cache = root.join("cache");
        fs::create_dir_all(repo.join("records")).unwrap();
        fs::create_dir_all(repo.join("definitions")).unwrap();
        let schema = br#"{"type":"object"}"#;
        fs::write(repo.join("definitions/input.authored.json"), schema).unwrap();
        let definition = serde_json::to_vec(&json!({"schema":"tig.definition.design-1",
            "id":"types/input","authored_schema":Object::from_bytes(schema),
            "authoring":{"properties":[],"required":[],"defaults":{}},
            "stream_rules":{},"semantic_rules":{}}))
        .unwrap();
        fs::write(repo.join("definitions/input.json"), &definition).unwrap();
        let definition_pin = Object::from_bytes(&definition);
        let mut logical = serde_json::to_value(&r).unwrap();
        for field in ["bank_audio", "calibration", "native_score_midi"] {
            let input = &mut logical[field];
            let bytes = fs::read(input["path"].as_str().unwrap()).unwrap();
            let hash = input["sha256"].as_str().unwrap();
            let target = cache.join("objects/sha256").join(&hash[..2]).join(hash);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, bytes).unwrap();
            let id = format!("inputs/{field}");
            let record = json!({"schema":"tig.instance.design-1","id":id,
                "definition":{"id":"types/input","sha256":definition_pin.sha256,"bytes":definition_pin.bytes},
                "authored":{},"selected":{"sha256":input["sha256"],"bytes":input["bytes"]},
                "streams":[],"status":{},"metadata":{}});
            tig_assets::native::Instance::parse(&serde_json::to_vec(&record).unwrap()).unwrap();
            fs::write(
                repo.join("records").join(format!("{field}.tig")),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            input["path"] = json!(id);
        }
        let git_executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("git.exe"))
            .find(|p| p.is_file())
            .unwrap()
            .canonicalize()
            .unwrap();
        let git = |args: &[&str]| {
            let result = Command::new(&git_executable)
                .current_dir(&repo)
                .args([
                    "-c",
                    "user.name=Synthetic fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "-c",
                    "core.hooksPath=NUL",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            String::from_utf8(result.stdout).unwrap().trim().to_string()
        };
        git(&["init", "--quiet"]);
        git(&["add", "records", "definitions"]);
        git(&["commit", "--quiet", "-m", "Synthetic sample inputs"]);
        let profile = json!({"schema":"reel.tig-input-profile.experimental-1",
            "repository":repo,"git_executable":git_executable,"commit":git(&["rev-parse","HEAD"]),
            "records":["records"],"definitions":["definitions"],
            "admitted":[{"id":"types/input","object":definition_pin,"payload":{"kind":"selected"}}],
            "byte_source":{"places":[{"root":cache,"layout":"reel-cas"}],"required":"prevent-mutation",
                "limits":{"object_bytes":131072,"total_bytes":1048576,"requests":8,"live_readers":4,"verify_millis":60000}}});
        let request_path = root.join("logical.json");
        fs::write(&request_path, serde_json::to_vec(&logical).unwrap()).unwrap();
        let render = |value: &Value, path: &Path, budget| {
            crate::tig_sample_clock::render(
                serde_json::from_value(value.clone()).unwrap(),
                &request_path,
                path,
                budget,
            )
        };
        let output = root.join("tig-render");
        render(&profile, &output, 5 * 1024 * 1024).unwrap();
        assert_eq!(
            fs::read(output.join("instrumental.wav")).unwrap(),
            fs::read(root.join("baseline/instrumental.wav")).unwrap()
        );
        let receipt: Value =
            serde_json::from_slice(&fs::read(output.join("tig-source.json")).unwrap()).unwrap();
        assert_eq!(receipt["inputs"].as_array().unwrap().len(), 3);
        assert_eq!(receipt["check"]["source_pins_verified"], true);
        assert_eq!(receipt["human_approval"], false);
        assert!(receipt["production_claim"].is_null());
        assert!(render(&profile, &output, 5 * 1024 * 1024).is_err());
        for (name, budget, within_cache) in [
            ("budget", 1, false),
            ("cache-output", 5 * 1024 * 1024, true),
        ] {
            let path = if within_cache {
                cache.join(name)
            } else {
                root.join(name)
            };
            assert!(render(&profile, &path, budget).is_err());
            assert!(!path.exists());
        }
        logical["bank_audio"]["sha256"] = json!("0".repeat(64));
        fs::write(&request_path, serde_json::to_vec(&logical).unwrap()).unwrap();
        assert!(render(&profile, &root.join("stale"), 5 * 1024 * 1024).is_err());
        assert!(!root.join("stale").exists());
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
    fn vocal_mix_preserves_clock_and_exact_quantized_stem_sum() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut r = request(root);
        r.vocal = Some(Vocal {
            source: input(root, "vocals.wav", &wave()),
            gain: 0.5,
        });
        r.peak_ceiling = 0.25;
        let summary = run(root, &r, "mix").unwrap();
        assert_eq!(summary["vocal_mixed"], true);
        let p = fs::read(root.join("mix/instrumental.wav")).unwrap();
        let v = fs::read(root.join("mix/vocal-stem.wav")).unwrap();
        let m = fs::read(root.join("mix/with-vocals.wav")).unwrap();
        assert_eq!(m.len(), 44 + 3200 * 2 * 3);
        for frame in 0..3200 {
            for channel in 0..2 {
                assert!(pcm(&m, frame, channel).abs() <= 2_097_152);
                assert_eq!(
                    pcm(&m, frame, channel),
                    pcm(&p, frame, channel) + pcm(&v, frame, channel)
                );
            }
        }
        assert_eq!(pcm(&p, 399, 0), 0);
        assert!(pcm(&v, 399, 0) > 0);
        assert_eq!(pcm(&p, 2400, 0), 0);
        assert!(pcm(&m, 2400, 0) > 0);
        let receipt: Value =
            serde_json::from_slice(&fs::read(root.join("mix/receipt.json")).unwrap()).unwrap();
        assert_eq!(receipt["vocal_retimed"], false);
        assert_eq!(receipt["vocal_regenerated"], false);
        assert!(receipt["attenuation"].as_f64().unwrap() < 1.0);
        let checked = check(&root.join("request.json"), &root.join("mix")).unwrap();
        assert_eq!(checked["exact_stem_sum_verified"], true);
        assert_eq!(checked["vocal_pcm_verified"], true);
        // Even self-consistent replacement hashes cannot hide a bad sum or
        // an altered vocal sample with the master changed to match it.
        for altered_vocal in [false, true] {
            let mut master = m.clone();
            let mut vocal = v.clone();
            let offset = 44 + 100 * 2 * 3;
            master[offset..offset + 3].copy_from_slice(&(pcm(&m, 100, 0) + 1).to_le_bytes()[..3]);
            if altered_vocal {
                vocal[offset..offset + 3]
                    .copy_from_slice(&(pcm(&v, 100, 0) + 1).to_le_bytes()[..3]);
            }
            fs::write(root.join("mix/with-vocals.wav"), &master).unwrap();
            fs::write(root.join("mix/vocal-stem.wav"), &vocal).unwrap();
            let mut changed = receipt.clone();
            changed["outputs"]["with-vocals.wav"]["sha256"] = json!(hash(&master));
            changed["outputs"]["vocal-stem.wav"]["sha256"] = json!(hash(&vocal));
            fs::write(
                root.join("mix/receipt.json"),
                serde_json::to_vec(&changed).unwrap(),
            )
            .unwrap();
            let error = check(&root.join("request.json"), &root.join("mix"))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(if altered_vocal {
                    "vocal PCM differs"
                } else {
                    "exact PCM24 stem sum"
                }),
                "{error}"
            );
        }
    }
    #[test]
    fn checker_accepts_prior_instrumental_packet_without_optional_voice_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let r = request(root);
        run(root, &r, "old").unwrap();
        let mut request: Value =
            serde_json::from_slice(&fs::read(root.join("request.json")).unwrap()).unwrap();
        request.as_object_mut().unwrap().remove("vocal");
        let request_raw = serde_json::to_vec(&request).unwrap();
        fs::write(root.join("request.json"), &request_raw).unwrap();
        let mut receipt: Value =
            serde_json::from_slice(&fs::read(root.join("old/receipt.json")).unwrap()).unwrap();
        for name in [
            "outputs",
            "vocal",
            "vocal_retimed",
            "vocal_regenerated",
            "vocal_pitched",
            "stem_sum_policy",
        ] {
            receipt.as_object_mut().unwrap().remove(name);
        }
        receipt["request_sha256"] = json!(hash(&request_raw));
        fs::write(
            root.join("old/receipt.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        let report = check(&root.join("request.json"), &root.join("old")).unwrap();
        assert_eq!(report["outputs_verified"], 1);
        assert_eq!(report["vocal_pcm_verified"], false);
    }
    #[test]
    fn wrong_vocal_clock_or_gain_refuses_before_output() {
        for case in ["rate", "frames", "gain"] {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            let mut r = request(root);
            let mut raw = wave();
            if case == "rate" {
                raw[24..28].copy_from_slice(&16000u32.to_le_bytes());
            }
            if case == "frames" {
                raw.extend([0; 8]);
                let size = raw.len() as u32 - 8;
                raw[4..8].copy_from_slice(&size.to_le_bytes());
                let size = raw.len() as u32 - 44;
                raw[40..44].copy_from_slice(&size.to_le_bytes());
            }
            r.vocal = Some(Vocal {
                source: input(root, "vocal.wav", &raw),
                gain: if case == "gain" { -1.0 } else { 1.0 },
            });
            assert!(run(root, &r, "refused-vocal").is_err());
            assert!(!root.join("refused-vocal").exists());
        }
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
