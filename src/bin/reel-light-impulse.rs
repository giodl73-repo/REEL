//! Render one data-driven light/recoil impulse across independently bound pictures.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Source {
    sha256: String,
    bytes: u64,
    cache_uri: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    id: String,
    source: Source,
    start_sample: u64,
    end_sample: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    peak: f64,
    hold_seconds: f64,
    decay_per_second: f64,
    end_seconds: f64,
    recoil_x_pixels: f64,
    recoil_y_pixels: f64,
    recoil_decay_per_second: f64,
    recoil_end_seconds: f64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    event_id: String,
    language: String,
    anchor_cue_id: String,
    sample_rate: u32,
    frame_rate: u32,
    width: u32,
    height: u32,
    onset_sample: u64,
    envelope: Envelope,
    segments: Vec<Segment>,
}
#[derive(Serialize)]
struct Output {
    segment_id: String,
    source: Source,
    start_sample: u64,
    end_sample: u64,
    start_frame: u64,
    end_frame: u64,
    frames: u64,
    path: String,
    sha256: String,
    bytes: u64,
    cache_uri: String,
    filter: String,
    decoded_frames: u64,
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
fn hash(path: &Path) -> Result<(String, u64)> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut n = 0;
    let mut b = [0u8; 65536];
    loop {
        let len = f.read(&mut b)?;
        if len == 0 {
            break;
        }
        h.update(&b[..len]);
        n += len as u64;
    }
    Ok((hex(h.finalize()), n))
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 180
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}
fn frames(m: &Manifest, s: &Segment) -> u64 {
    frame_boundary(m, s.end_sample) - frame_boundary(m, s.start_sample)
}
fn frame_boundary(m: &Manifest, sample: u64) -> u64 {
    ((sample as u128 * m.frame_rate as u128).div_ceil(m.sample_rate as u128)) as u64
}
fn validate(m: &Manifest) -> Result<()> {
    if m.schema != "reel.light-impulse.v1"
        || !valid_id(&m.event_id)
        || !valid_id(&m.anchor_cue_id)
        || !valid_id(&m.language)
        || !(8000..=192000).contains(&m.sample_rate)
        || !(1..=120).contains(&m.frame_rate)
        || !(16..=3840).contains(&m.width)
        || !(16..=2160).contains(&m.height)
        || m.width % 2 != 0
        || m.height % 2 != 0
        || m.segments.is_empty()
        || m.segments.len() > 32
    {
        bail!("invalid light impulse identity, clock or dimensions")
    }
    let e = &m.envelope;
    if [
        e.peak,
        e.hold_seconds,
        e.decay_per_second,
        e.end_seconds,
        e.recoil_x_pixels,
        e.recoil_y_pixels,
        e.recoil_decay_per_second,
        e.recoil_end_seconds,
    ]
    .iter()
    .any(|x| !x.is_finite())
        || !(0.0..=1.0).contains(&e.peak)
        || !(0.0..=1.0).contains(&e.hold_seconds)
        || !(0.01..=100.0).contains(&e.decay_per_second)
        || e.end_seconds <= e.hold_seconds
        || e.end_seconds > 15.0
        || !(0.0..=32.0).contains(&e.recoil_x_pixels)
        || !(0.0..=32.0).contains(&e.recoil_y_pixels)
        || !(0.01..=100.0).contains(&e.recoil_decay_per_second)
        || !(0.0..=3.0).contains(&e.recoil_end_seconds)
    {
        bail!("invalid finite impulse envelope")
    }
    let mut ids = BTreeSet::new();
    let mut end = 0;
    for s in &m.segments {
        if !valid_id(&s.id)
            || !ids.insert(&s.id)
            || s.start_sample != end
            || s.end_sample <= s.start_sample
            || s.source.sha256.len() != 64
            || !s
                .source
                .sha256
                .bytes()
                .all(|x| x.is_ascii_hexdigit() && !x.is_ascii_uppercase())
            || s.source.bytes == 0
            || frames(m, s) == 0
            || s.source.cache_uri != format!("cache://sha256/{}", s.source.sha256)
        {
            bail!("invalid, noncontiguous or duplicate segment")
        }
        end = s.end_sample;
    }
    if m.onset_sample >= end || end > m.sample_rate as u64 * 60 {
        bail!("impulse onset outside bounded sequence")
    }
    Ok(())
}
fn filter(m: &Manifest, s: &Segment) -> String {
    let e = &m.envelope;
    // Signed difference is deliberate: later segments continue the same impulse.
    let offset = frame_boundary(m, s.start_sample) as f64 / m.frame_rate as f64
        - m.onset_sample as f64 / m.sample_rate as f64;
    let d = format!("(t+{offset:.12})");
    let taper = (e.end_seconds - e.hold_seconds).min(1.0);
    let brightness = format!(
        "if(between({d},0,{end}),{peak}*exp(-{decay}*max({d}-{hold},0))*min(1,max(0,({end}-{d})/{taper})),0)",
        end = e.end_seconds,
        peak = e.peak,
        decay = e.decay_per_second,
        hold = e.hold_seconds
    );
    let x = format!(
        "32+if(between({d},0,{end}),{amount}*exp(-{decay}*{d})*sin(65*{d}),0)",
        end = e.recoil_end_seconds,
        amount = e.recoil_x_pixels,
        decay = e.recoil_decay_per_second
    );
    let y = format!(
        "32+if(between({d},0,{end}),{amount}*exp(-{decay}*{d})*sin(55*{d}),0)",
        end = e.recoil_end_seconds,
        amount = e.recoil_y_pixels,
        decay = e.recoil_decay_per_second
    );
    format!(
        "scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},pad={pw}:{ph}:32:32:color=black,fillborders=left=32:right=32:top=32:bottom=32:mode=smear,crop={w}:{h}:x='{x}':y='{y}',eq=brightness='{brightness}':eval=frame,format=yuva444p",
        w = m.width,
        h = m.height,
        pw = m.width + 64,
        ph = m.height + 64
    )
}
fn source_path(root: &Path, s: &Source) -> PathBuf {
    root.join("objects/sha256")
        .join(&s.sha256[..2])
        .join(&s.sha256)
}
fn run(manifest: &Path, root: &Path, out: &Path) -> Result<()> {
    let bytes = fs::read(manifest)?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    validate(&m)?;
    for s in &m.segments {
        if hash(&source_path(root, &s.source))? != (s.source.sha256.clone(), s.source.bytes) {
            bail!("input hash/bytes mismatch: {}", s.id)
        }
    }
    if out.exists() {
        bail!("output directory already exists; retain it and choose a new job")
    }
    fs::create_dir_all(out)?;
    let mut outputs = Vec::new();
    for (i, s) in m.segments.iter().enumerate() {
        let path = out.join(format!("segment-{i:03}.mkv"));
        let vf = filter(&m, s);
        let count = frames(&m, s);
        let status = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-n",
                "-loop",
                "1",
                "-framerate",
                &m.frame_rate.to_string(),
                "-i",
            ])
            .arg(source_path(root, &s.source))
            .args([
                "-frames:v",
                &count.to_string(),
                "-vf",
                &vf,
                "-c:v",
                "ffv1",
                "-level",
                "3",
                "-threads",
                "2",
                "-pix_fmt",
                "yuva444p",
                "-an",
            ])
            .arg(&path)
            .status()
            .context("start FFmpeg")?;
        if !status.success() {
            bail!("effect rendering failed for {}", s.id)
        }
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_frames",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=nb_read_frames,width,height,pix_fmt",
                "-of",
                "json",
            ])
            .arg(&path)
            .output()?;
        if !probe.status.success() {
            bail!("effect decode/probe failed")
        }
        let p: serde_json::Value = serde_json::from_slice(&probe.stdout)?;
        let v = &p["streams"][0];
        let decoded = v["nb_read_frames"]
            .as_str()
            .context("frame count")?
            .parse::<u64>()?;
        if decoded != count
            || v["width"].as_u64() != Some(m.width as u64)
            || v["height"].as_u64() != Some(m.height as u64)
            || v["pix_fmt"] != "yuva444p"
        {
            bail!("effect format/frame validation failed")
        }
        let decode = Command::new("ffmpeg")
            .args(["-v", "error", "-xerror", "-nostdin", "-i"])
            .arg(&path)
            .args(["-map", "0:v:0", "-f", "null", "-"])
            .status()?;
        if !decode.success() {
            bail!("full effect decode failed")
        }
        let (h, n) = hash(&path)?;
        outputs.push(Output {
            segment_id: s.id.clone(),
            source: s.source.clone(),
            start_sample: s.start_sample,
            end_sample: s.end_sample,
            start_frame: frame_boundary(&m, s.start_sample),
            end_frame: frame_boundary(&m, s.end_sample),
            frames: count,
            path: path.file_name().unwrap().to_string_lossy().into_owned(),
            cache_uri: format!("cache://sha256/{h}"),
            sha256: h,
            bytes: n,
            filter: vf,
            decoded_frames: decoded,
        });
    }
    let receipt = serde_json::json!({"schema":"reel.light-impulse-receipt.v1","event_id":m.event_id,"language":m.language,"anchor_cue_id":m.anchor_cue_id,"onset_sample":m.onset_sample,"sample_rate":m.sample_rate,"frame_rate":m.frame_rate,"event_count":1,"manifest_sha256":hex(Sha256::digest(&bytes)),"manifest_bytes":bytes.len(),"state":"rendered-fully-decoded","outputs":outputs,"creative_authority":"not-granted-by-this-receipt","limits":"Opaque picture-derived VFX assets; source pictures unchanged. Native scene consumption, sound balance, human review and cache registration remain separate."});
    fs::write(
        out.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(())
}
fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().skip(1).collect();
    match a.as_slice() {
        [command, manifest, root, output] if command == "build" => {
            run(Path::new(manifest), Path::new(root), Path::new(output))
        }
        [command, manifest] if command == "validate" => {
            let m: Manifest = serde_json::from_slice(&fs::read(manifest)?)?;
            validate(&m)
        }
        _ => bail!(
            "usage: reel-light-impulse validate <manifest.json> | build <manifest.json> <asset-root> <new-output-dir>"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Manifest {
        serde_json::from_value(serde_json::json!({"schema":"reel.light-impulse.v1","event_id":"one-impact","language":"es","anchor_cue_id":"impact-word","sample_rate":44100,"frame_rate":24,"width":64,"height":36,"onset_sample":11025,"envelope":{"peak":0.9,"hold_seconds":0.125,"decay_per_second":1.6,"end_seconds":1.5,"recoil_x_pixels":0.0,"recoil_y_pixels":0.0,"recoil_decay_per_second":5.0,"recoil_end_seconds":0.8},"segments":[{"id":"first","source":{"sha256":"a".repeat(64),"bytes":1,"cache_uri":format!("cache://sha256/{}","a".repeat(64))},"start_sample":0,"end_sample":44100},{"id":"second","source":{"sha256":"a".repeat(64),"bytes":1,"cache_uri":format!("cache://sha256/{}","a".repeat(64))},"start_sample":44100,"end_sample":88200}]})).unwrap()
    }
    #[test]
    fn continuous_clock_and_integer_frames() {
        let m = fixture();
        validate(&m).unwrap();
        assert_eq!(frames(&m, &m.segments[0]), 24);
        assert!(filter(&m, &m.segments[0]).contains("t+-0.250000000000"));
        assert!(filter(&m, &m.segments[1]).contains("t+0.750000000000"));
    }
    #[test]
    fn fractional_cuts_share_one_frame_clock() {
        let mut m = fixture();
        m.onset_sample = 0;
        m.segments[0].end_sample = 2205;
        m.segments[1].start_sample = 2205;
        m.segments[1].end_sample = 4410;
        validate(&m).unwrap();
        assert_eq!(frames(&m, &m.segments[0]), 2);
        assert_eq!(frames(&m, &m.segments[1]), 1);
        assert_eq!(frame_boundary(&m, 4410), 3);
        assert!(filter(&m, &m.segments[1]).contains("t+0.083333333333"));
    }
    #[test]
    fn rejects_bad_clocks_repeated_ids_and_unbounded_effects() {
        let mut m = fixture();
        m.segments[1].start_sample = 44099;
        assert!(validate(&m).is_err());
        m = fixture();
        m.segments[1].id = "first".into();
        assert!(validate(&m).is_err());
        m = fixture();
        m.envelope.peak = f64::NAN;
        assert!(validate(&m).is_err());
        m = fixture();
        m.onset_sample = 88200;
        assert!(validate(&m).is_err());
    }
    #[test]
    fn actual_render_continues_and_returns_to_dark() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let mut m = fixture();
        let mut image = b"P6\n64 36\n255\n".to_vec();
        image.extend(vec![8u8; 64 * 36 * 3]);
        let h = hex(Sha256::digest(&image));
        let source = Source {
            sha256: h.clone(),
            bytes: image.len() as u64,
            cache_uri: format!("cache://sha256/{h}"),
        };
        let p = source_path(root, &source);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, image).unwrap();
        for s in &mut m.segments {
            s.source = source.clone();
        }
        let manifest = root.join("fixture.json");
        fs::write(&manifest, serde_json::to_vec(&m).unwrap()).unwrap();
        let out = root.join("output");
        run(&manifest, root, &out).unwrap();
        let mean = |index: u8, time: &str| {
            let o = Command::new("ffmpeg")
                .args(["-v", "error", "-ss", time, "-i"])
                .arg(out.join(format!("segment-{index:03}.mkv")))
                .args(["-frames:v", "1", "-pix_fmt", "gray", "-f", "rawvideo", "-"])
                .output()
                .unwrap();
            assert!(o.status.success());
            o.stdout.iter().map(|x| *x as f64).sum::<f64>() / o.stdout.len() as f64
        };
        let before = mean(0, "0");
        let peak = mean(0, "0.333333");
        let continued = mean(1, "0");
        let after = mean(1, "0.958333");
        assert!(peak > before + 150.0);
        assert!(continued > before + 15.0 && continued < peak - 80.0);
        assert!((after - before).abs() < 2.0);
        assert!(run(&manifest, root, &out).is_err());
    }
}
