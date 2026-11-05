//! Bounded label/reading-page projections; the complete MP3 packets are copied.
use super::{
    Input, Object, Permissions, RATE, capture, hold, identity, label, object, packet_path,
    request_directory, sha256_bytes, write_new,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
mod layout;
const MAX_JSON: u64 = 4 * 1024 * 1024;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Font {
    file: Input,
    family: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Palette {
    paper: String,
    ink: String,
    accent: String,
    secondary: String,
    rule: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Style {
    body_family: String,
    heading_family: String,
    heading_bold: bool,
    track_title_bold: bool,
    footer: String,
    byline: String,
    left_heading: String,
    right_heading: String,
    instrumental_left: String,
    instrumental_right: String,
    wordless_left: String,
    wordless_right: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    max_input_bytes: u64,
    max_output_bytes: u64,
    timeout_seconds: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    title: String,
    ffmpeg: Input,
    ffprobe: Input,
    audio: Input,
    chapters: Input,
    album_receipt: Input,
    album_request: Option<Input>,
    reading_plan: Option<Input>,
    fonts: Vec<Font>,
    style: Style,
    palette: Option<Palette>,
    permissions: Permissions,
    limits: Limits,
}
#[derive(Serialize)]
struct Track {
    id: String,
    title: String,
    start: u64,
    end: u64,
    source_end: u64,
}
#[derive(Serialize)]
struct Album {
    title: String,
    frames: u64,
    tracks: Vec<Track>,
}
fn pin(input: &Input) -> Object {
    Object {
        sha256: input.sha256.clone(),
        bytes: input.bytes,
    }
}
fn json_input(input: &Input) -> Result<Value> {
    if input.bytes > MAX_JSON {
        bail!("album video metadata budget");
    }
    let mut raw = Vec::new();
    File::open(&input.path)?
        .take(input.bytes + 1)
        .read_to_end(&mut raw)?;
    if raw.len() as u64 != input.bytes || sha256_bytes(&raw) != input.sha256 {
        bail!("album video metadata identity differs");
    }
    crate::approval_attestation::ensure_no_duplicate_keys(&raw)?;
    Ok(serde_json::from_slice(&raw)?)
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().context("album video text missing")
}
fn number(v: &Value, key: &str) -> Result<u64> {
    v[key].as_u64().context("album video integer clock missing")
}
fn resolve_input(input: &mut Input, base: &Path) -> Result<()> {
    if !input.path.is_absolute() {
        if input.path.as_os_str().is_empty()
            || !input
                .path
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_)))
        {
            bail!("album video input escapes workspace");
        }
        let resolved = base.join(&input.path).canonicalize()?;
        if !resolved.starts_with(base) {
            bail!("album video input escaped workspace");
        }
        input.path = resolved;
    }
    Ok(())
}
type Loaded = (Request, Vec<u8>, Vec<File>, Album, Option<Value>);
fn load(path: &Path) -> Result<Loaded> {
    let mut raw = Vec::new();
    File::open(path)?.take(MAX_JSON + 1).read_to_end(&mut raw)?;
    if raw.len() as u64 > MAX_JSON {
        bail!("album video request budget");
    }
    crate::approval_attestation::ensure_no_duplicate_keys(&raw)?;
    let mut q: Request = serde_json::from_slice(&raw)?;
    if q.schema != "reel.album-video.v1"
        || !q.permissions.private_only
        || q.permissions.external_upload
        || q.permissions.public_release
        || !label(&q.title)
        || !q.ffmpeg.path.is_absolute()
        || !q.ffprobe.path.is_absolute()
        || q.fonts.is_empty()
        || q.fonts.len() > 8
        || !(1..=3600).contains(&q.limits.timeout_seconds)
        || q.limits.max_output_bytes > 32 * 1024 * 1024 * 1024
        || q.limits.max_input_bytes > 32 * 1024 * 1024 * 1024
    {
        bail!("unsupported album video scope/budget");
    }
    let base = request_directory(path)?;
    let mut inputs = vec![&mut q.audio, &mut q.chapters, &mut q.album_receipt];
    inputs.extend(q.album_request.iter_mut());
    inputs.extend(q.reading_plan.iter_mut());
    inputs.extend(q.fonts.iter_mut().map(|font| &mut font.file));
    for input in inputs {
        resolve_input(input, &base)?;
    }
    let mut all = vec![
        &q.ffmpeg,
        &q.ffprobe,
        &q.audio,
        &q.chapters,
        &q.album_receipt,
    ];
    all.extend(q.album_request.iter());
    all.extend(q.reading_plan.iter());
    all.extend(q.fonts.iter().map(|f| &f.file));
    let total = all
        .iter()
        .try_fold(0u64, |sum, input| sum.checked_add(input.bytes))
        .context("album video input size overflow")?;
    if total > q.limits.max_input_bytes
        || q.audio.bytes + 2 * MAX_JSON + q.fonts.iter().map(|f| f.file.bytes).sum::<u64>()
            > q.limits.max_output_bytes
    {
        bail!("album video byte allowance insufficient");
    }
    let mut held = Vec::new();
    for input in all {
        held.push(hold(input)?);
    }
    let album = album(&q)?;
    let reading = q.reading_plan.as_ref().map(json_input).transpose()?;
    let _ = layout::compile(&q, &album, reading.as_ref())?;
    Ok((q, raw, held, album, reading))
}
fn album(q: &Request) -> Result<Album> {
    let v = json_input(&q.album_receipt)?;
    let schema = text(&v, "schema")?;
    let title = match schema {
        "reel.music-album-receipt.v1" => {
            let input = q
                .album_request
                .as_ref()
                .context("native album request required for its title/source binding")?;
            if serde_json::from_value::<Object>(v["request"].clone())? != pin(input) {
                bail!("native album request differs");
            }
            let original = json_input(input)?;
            text(&original, "title")?.to_owned()
        }
        "reel.local-song-album-master-check.v1" => text(&v, "title")?.to_owned(),
        _ => bail!("unsupported retained album ledger"),
    };
    if title != q.title || number(&v, "sample_rate")? != RATE || number(&v, "channels")? != 2 {
        bail!("album title/audio standard differs");
    }
    if serde_json::from_value::<Object>(v["outputs"]["master.mp3"].clone())? != pin(&q.audio)
        || serde_json::from_value::<Object>(v["outputs"]["chapters.ffmetadata"].clone())?
            != pin(&q.chapters)
    {
        bail!("album video inputs differ from retained ledger");
    }
    let frames = number(&v, "frames")?;
    let list = v["tracks"].as_array().context("album tracks absent")?;
    if !(RATE..=14400 * RATE).contains(&frames) || list.is_empty() || list.len() > 128 {
        bail!("album video extent/track budget");
    }
    let mut tracks = Vec::new();
    let mut ids = BTreeSet::new();
    for (i, t) in list.iter().enumerate() {
        let id = t
            .get("asset_id")
            .or_else(|| t.get("song_id"))
            .and_then(Value::as_str)
            .context("album track ID missing")?
            .to_owned();
        let start = number(t, "start_frame")?;
        let end = if i + 1 < list.len() {
            number(&list[i + 1], "start_frame")?
        } else {
            frames
        };
        let source_end = if let Some(n) = t.get("frames") {
            start
                .checked_add(n.as_u64().context("invalid source duration")?)
                .context("source clock overflow")?
        } else {
            end
        };
        let title = text(t, "title")?.to_owned();
        if !label(&id)
            || !label(&title)
            || !ids.insert(id.clone())
            || (i == 0 && start != 0)
            || start >= end
            || end > frames
            || source_end > end
            || source_end <= start
        {
            bail!("album track order/clock invalid");
        }
        tracks.push(Track {
            id,
            title,
            start,
            end,
            source_end,
        });
    }
    Ok(Album {
        title,
        frames,
        tracks,
    })
}
fn cmd(q: &Request) -> Command {
    let mut c = Command::new(&q.ffmpeg.path);
    c.args(["-nostdin", "-hide_banner", "-nostats", "-n"]);
    c
}
fn media_input(c: &mut Command, path: &Path) {
    c.args([
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        "mp3,mov",
        "-i",
    ])
    .arg(path);
}
fn probe(q: &Request, path: &Path, deadline: Instant) -> Result<Value> {
    let mut c = Command::new(&q.ffprobe.path);
    c.args([
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        "mp3,mov",
        "-count_packets",
        "-show_streams",
        "-show_format",
        "-show_chapters",
        "-of",
        "json",
    ])
    .arg(path);
    let (raw, _) = capture(&mut c, MAX_JSON as usize, deadline)?;
    Ok(serde_json::from_slice(&raw)?)
}
fn packets(q: &Request, path: &Path, deadline: Instant) -> Result<Vec<u8>> {
    let mut c = cmd(q);
    c.args(["-v", "error"]);
    media_input(&mut c, path);
    c.args([
        "-map", "0:a:0", "-c:a", "copy", "-f", "hash", "-hash", "sha256", "-",
    ]);
    let (raw, _) = capture(&mut c, 1024, deadline)?;
    Ok(raw)
}
fn leading_skip(q: &Request, path: &Path, deadline: Instant) -> Result<u64> {
    let mut c = Command::new(&q.ffprobe.path);
    c.args([
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        "mp3,mov",
        "-select_streams",
        "a:0",
        "-read_intervals",
        "%+#1",
        "-show_packets",
        "-of",
        "json",
    ])
    .arg(path);
    let (raw, _) = capture(&mut c, MAX_JSON as usize, deadline)?;
    let v: Value = serde_json::from_slice(&raw)?;
    let packets = v["packets"]
        .as_array()
        .context("first audio packet absent")?;
    if packets.len() != 1 {
        bail!("first audio packet observation differs");
    }
    Ok(packets[0]["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["side_data_type"] == "Skip Samples")
        .and_then(|s| s["skip_samples"].as_u64())
        .unwrap_or(0))
}
fn pcm(q: &Request, path: &Path, frames: u64, deadline: Instant) -> Result<(Object, u64)> {
    let mut c = cmd(q);
    c.args(["-v", "error"]);
    media_input(&mut c, path);
    c.args([
        "-map", "0:a:0", "-ar", "48000", "-ac", "2", "-f", "f32le", "-",
    ]);
    let (value, _) = super::process::run(&mut c, deadline, move |mut stdout| {
        let canonical = identity(
            std::io::Read::by_ref(&mut stdout).take(frames * 8),
            frames * 8,
        )?;
        let tail = identity(stdout, RATE / 20 * 8)?;
        let bytes = canonical.bytes + tail.bytes;
        Ok((canonical, bytes))
    })?;
    Ok(value)
}
fn verify(
    q: &Request,
    a: &Album,
    output: &Path,
    deadline: Instant,
    width: u32,
    height: u32,
) -> Result<Value> {
    let source = probe(q, &q.audio.path, deadline)?;
    let movie = probe(q, &output.join("master.mp4"), deadline)?;
    let streams = movie["streams"]
        .as_array()
        .context("video streams missing")?;
    let videos: Vec<_> = streams
        .iter()
        .filter(|s| s["codec_type"] == "video")
        .collect();
    let sounds: Vec<_> = streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .collect();
    let input_sounds: Vec<_> = source["streams"]
        .as_array()
        .context("source streams missing")?
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .collect();
    if videos.len() != 1
        || sounds.len() != 1
        || input_sounds.len() != 1
        || videos[0]["codec_name"] != "h264"
        || sounds[0]["codec_name"] != "mp3"
        || input_sounds[0]["codec_name"] != "mp3"
        || videos[0]["width"] != width
        || videos[0]["height"] != height
        || videos[0]["avg_frame_rate"] != "5/1"
        || sounds[0]["sample_rate"] != "48000"
        || sounds[0]["channels"] != 2
        || input_sounds[0]["nb_read_packets"] != sounds[0]["nb_read_packets"]
    {
        bail!("album video codec/frame/audio packet contract differs");
    }
    if videos[0]["start_pts"].as_i64() != Some(0)
        || sounds[0]["start_pts"].as_i64() != Some(0)
        || sounds[0]["time_base"] != "1/48000"
        || sounds[0]["duration_ts"].as_u64() != Some(a.frames)
    {
        bail!("album video timeline must match the exact zero-based album sample clock");
    }
    let chapters = movie["chapters"]
        .as_array()
        .context("video chapters absent")?;
    if chapters.len() != a.tracks.len() {
        bail!("video chapter count differs");
    }
    for (chapter, track) in chapters.iter().zip(&a.tracks) {
        let start: f64 = text(chapter, "start_time")?.parse()?;
        let end: f64 = text(chapter, "end_time")?.parse()?;
        if !start.is_finite()
            || !end.is_finite()
            || (start - track.start as f64 / RATE as f64).abs() > 0.002
            || end <= start
            || (end - track.source_end as f64 / RATE as f64).abs() > 0.002
            || chapter["tags"]["title"] != track.title
        {
            bail!("video chapter identity/clock differs");
        }
    }
    let duration: f64 = text(&movie["format"], "duration")?.parse()?;
    if !duration.is_finite()
        || duration < a.frames as f64 / RATE as f64 - 0.05
        || duration > a.frames as f64 / RATE as f64 + 0.3
    {
        bail!("video altered complete album duration");
    }
    if packets(q, &q.audio.path, deadline)? != packets(q, &output.join("master.mp4"), deadline)? {
        bail!("video altered encoded audio packets/order");
    }
    // MPEG-1 Layer III at 48 kHz has exactly 1,152 samples per packet.
    // Older MOV demuxers expose the final discard padding despite a correct
    // sample-exact edit-list duration. Prove that bound from unchanged packets
    // and identical leading skip, then compare every sample on the album clock.
    let count: u64 = text(sounds[0], "nb_read_packets")?.parse()?;
    let source_skip = leading_skip(q, &q.audio.path, deadline)?;
    if leading_skip(q, &output.join("master.mp4"), deadline)? != source_skip {
        bail!("video changed encoder priming");
    }
    let padding = count
        .checked_mul(1152)
        .and_then(|n| n.checked_sub(source_skip))
        .and_then(|n| n.checked_sub(a.frames))
        .filter(|n| *n < 1152)
        .context("source codec padding is not bounded")?;
    let (source_pcm, source_bytes) = pcm(q, &q.audio.path, a.frames, deadline)?;
    let (movie_pcm, movie_bytes) = pcm(q, &output.join("master.mp4"), a.frames, deadline)?;
    if source_pcm.bytes != a.frames * 8
        || source_bytes != source_pcm.bytes
        || movie_pcm != source_pcm
        || ![a.frames * 8, (a.frames + padding) * 8].contains(&movie_bytes)
    {
        bail!(
            "video changed decoded audio or codec padding: source {} frames, movie {} frames, PCM hashes equal {}",
            source_bytes / 8,
            movie_bytes / 8,
            movie_pcm.sha256 == source_pcm.sha256
        );
    }
    let mut c = cmd(q);
    c.args(["-v", "error"]);
    media_input(&mut c, &output.join("master.mp4"));
    c.args(["-f", "null", "-"]);
    capture(&mut c, 0, deadline)?;
    Ok(
        json!({"audio_packets":sounds[0]["nb_read_packets"],"audio_packet_payloads_exact":true,"decoded_pcm":source_pcm,
        "source_discard_padding_frames":padding,"decoder_exposed_padding_frames":movie_bytes/8-a.frames,
        "complete_decode":true,"audio_reencoded":false,"picture_source_correspondence":false,"human_approval":false,"published":false}),
    )
}
pub fn plan(path: &Path) -> Result<Value> {
    let (q, raw, _held, a, reading) = load(path)?;
    let c = layout::compile(&q, &a, reading.as_ref())?;
    Ok(
        json!({"schema":"reel.album-video-plan.v1","request_sha256":sha256_bytes(&raw),"canvas":[c.width,c.height],"fps":5,"video_frames":(a.frames*5).div_ceil(RATE),"audio_frames":a.frames,"layout":c.ledger,"media_executed":false,"max_output_bytes":q.limits.max_output_bytes}),
    )
}
fn outputs(output: &Path, max: u64) -> Result<BTreeMap<String, Object>> {
    let mut result = BTreeMap::new();
    for name in ["master.mp4", "labels.ass", "layout.json"] {
        result.insert(name.into(), object(&output.join(name), max)?);
    }
    Ok(result)
}
fn packet_usage(q: &Request, output: &Path, with_fonts: bool) -> Result<()> {
    let mut total = 0u64;
    for entry in fs::read_dir(output)? {
        let entry = entry?;
        let name = entry.file_name();
        let kind = entry.file_type()?;
        if name == "fonts" && with_fonts && kind.is_dir() {
            for font in fs::read_dir(entry.path())? {
                let font = font?;
                let allowed = (0..q.fonts.len())
                    .any(|i| font.file_name() == format!("face-{i}.ttf").as_str());
                if !allowed || !font.file_type()?.is_file() {
                    bail!("unexpected owned font scratch");
                }
                total = total
                    .checked_add(font.metadata()?.len())
                    .context("video size overflow")?;
            }
        } else {
            if !kind.is_file()
                || !name.to_str().is_some_and(|s| {
                    matches!(
                        s,
                        "master.mp4" | "labels.ass" | "layout.json" | "receipt.json"
                    )
                })
            {
                bail!("unexpected album video packet entry");
            }
            total = total
                .checked_add(entry.metadata()?.len())
                .context("video size overflow")?;
        }
    }
    if total > q.limits.max_output_bytes {
        bail!("album video packet byte budget exceeded");
    }
    Ok(())
}
pub fn render(request: &Path, output: &Path) -> Result<Value> {
    let (q, raw, _held, a, reading) = load(request)?;
    let output = packet_path(request, output, false)?;
    let c = layout::compile(&q, &a, reading.as_ref())?;
    fs::create_dir(&output).context("album video output must be new")?;
    let deadline = Instant::now() + Duration::from_secs(q.limits.timeout_seconds);
    write_new(&output.join("labels.ass"), c.ass.as_bytes())?;
    write_new(
        &output.join("layout.json"),
        &serde_json::to_vec_pretty(&c.ledger)?,
    )?;
    let fonts = output.join("fonts");
    fs::create_dir(&fonts)?;
    let mut font_paths = Vec::new();
    for (i, font) in q.fonts.iter().enumerate() {
        let path = fonts.join(format!("face-{i}.ttf"));
        let raw = fs::read(&font.file.path)?;
        write_new(&path, &raw)?;
        font_paths.push((path, pin(&font.file)));
    }
    let used = font_paths.iter().map(|(_, p)| p.bytes).sum::<u64>() + c.ass.len() as u64 + MAX_JSON;
    let remaining = q
        .limits
        .max_output_bytes
        .checked_sub(used)
        .context("video budget exhausted by layout/fonts")?;
    let mut command = cmd(&q);
    command
        .current_dir(&output)
        .args(["-v", "error", "-f", "lavfi", "-i"])
        .arg(format!(
            "color=c=0x{}:s={}x{}:r=5:d={}",
            c.paper,
            c.width,
            c.height,
            (a.frames * 5).div_ceil(RATE) as f64 / 5.0
        ));
    media_input(&mut command, &q.audio.path);
    command
        .args(["-f", "ffmetadata", "-i"])
        .arg(&q.chapters.path)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-vf",
            "ass=labels.ass:fontsdir=fonts",
            "-map_metadata",
            "2",
            "-map_chapters",
            "2",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "20",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "copy",
            "-movflags",
            "+faststart",
            "-video_track_timescale",
            "48000",
            "-movie_timescale",
            "48000",
            "-fs",
        ])
        .arg(remaining.to_string())
        .arg("master.mp4");
    capture(&mut command, 0, deadline)?;
    packet_usage(&q, &output, true)?;
    let observation = verify(&q, &a, &output, deadline, c.width, c.height)?;
    let artifact_pins = outputs(&output, q.limits.max_output_bytes)?;
    let receipt = json!({"schema":"reel.album-video-receipt.v1","request":{"sha256":sha256_bytes(&raw),"bytes":raw.len()},"outputs":artifact_pins,"observation":observation,"human_approval":false,"published":false});
    write_new(
        &output.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    for (path, pin) in font_paths {
        if object(&path, pin.bytes)? != pin {
            bail!("owned font scratch changed");
        }
        fs::remove_file(path)?;
    }
    fs::remove_dir(fonts)?;
    packet_usage(&q, &output, false)?;
    Ok(receipt)
}
pub fn check(request: &Path, output: &Path) -> Result<Value> {
    let (q, raw, _held, a, reading) = load(request)?;
    let output = packet_path(request, output, true)?;
    packet_usage(&q, &output, false)?;
    let c = layout::compile(&q, &a, reading.as_ref())?;
    let pin_receipt = object(&output.join("receipt.json"), MAX_JSON)?;
    let receipt = json_input(&Input {
        path: output.join("receipt.json"),
        sha256: pin_receipt.sha256,
        bytes: pin_receipt.bytes,
    })?;
    if receipt["schema"] != "reel.album-video-receipt.v1"
        || receipt["request"]["sha256"] != sha256_bytes(&raw)
        || receipt["request"]["bytes"] != raw.len()
        || receipt["human_approval"] != false
        || receipt["published"] != false
        || serde_json::from_value::<BTreeMap<String, Object>>(receipt["outputs"].clone())?
            != outputs(&output, q.limits.max_output_bytes)?
    {
        bail!("album video receipt/output differs");
    }
    if fs::read(output.join("labels.ass"))? != c.ass.as_bytes()
        || serde_json::from_slice::<Value>(&fs::read(output.join("layout.json"))?)? != c.ledger
    {
        bail!("album video layout differs from sources");
    }
    verify(
        &q,
        &a,
        &output,
        Instant::now() + Duration::from_secs(q.limits.timeout_seconds),
        c.width,
        c.height,
    )
}
#[cfg(test)]
mod tests;
