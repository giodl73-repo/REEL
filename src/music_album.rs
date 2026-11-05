//! Full-recording fixed-gain album packets. No discovery, model, Python or upload.
use anyhow::{Context, Result, bail};
use reel_music::hash::sha256_bytes;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
mod process;
const RATE: u64 = 48000;
const REQUEST_MAX: u64 = 524288;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Object {
    sha256: String,
    bytes: u64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Input {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Permissions {
    private_only: bool,
    external_upload: bool,
    public_release: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Track {
    asset_id: String,
    revision: String,
    title: String,
    source: Input,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    max_input_bytes: u64,
    max_track_frames: u64,
    max_total_frames: u64,
    max_output_bytes: u64,
    timeout_seconds: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    title: String,
    delivery_stem: String,
    ffmpeg: Input,
    permissions: Permissions,
    gap_frames: u64,
    target_lufs: f64,
    source_peak_ceiling_dbtp: f64,
    limits: Limits,
    tracks: Vec<Track>,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn identity(mut reader: impl Read, maximum: u64) -> Result<Object> {
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 65536];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes = bytes.checked_add(n as u64).context("album byte overflow")?;
        if bytes > maximum {
            bail!("album object exceeds byte budget");
        }
        hash.update(&buffer[..n]);
    }
    Ok(Object {
        sha256: hex(&hash.finalize()),
        bytes,
    })
}
fn object(path: &Path, maximum: u64) -> Result<Object> {
    identity(File::open(path)?, maximum)
}
fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn hold(input: &Input) -> Result<File> {
    if !input.path.is_absolute()
        || !digest_valid(&input.sha256)
        || input.bytes == 0
        || input.bytes > 2 * 1024 * 1024 * 1024
    {
        bail!("invalid declared album input");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let file = options.open(&input.path)?;
    if !file.metadata()?.is_file()
        || identity(file.try_clone()?, input.bytes)?
            != (Object {
                sha256: input.sha256.clone(),
                bytes: input.bytes,
            })
    {
        bail!("album input identity differs");
    }
    Ok(file)
}
fn label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(|c| c.is_control())
}
fn load(path: &Path) -> Result<(Request, Vec<u8>)> {
    let mut raw = Vec::new();
    File::open(path)?
        .take(REQUEST_MAX + 1)
        .read_to_end(&mut raw)?;
    if raw.len() as u64 > REQUEST_MAX {
        bail!("album request budget");
    }
    crate::approval_attestation::ensure_no_duplicate_keys(&raw)?;
    let mut q: Request = serde_json::from_slice(&raw)?;
    validate(&q)?;
    let base = request_directory(path)?;
    for track in &mut q.tracks {
        if !track.source.path.is_absolute() {
            if track.source.path.as_os_str().is_empty()
                || !track
                    .source
                    .path
                    .components()
                    .all(|component| matches!(component, std::path::Component::Normal(_)))
            {
                bail!("relative album source must remain beneath the request workspace");
            }
            let resolved = base.join(&track.source.path).canonicalize()?;
            if !resolved.starts_with(&base) {
                bail!("relative album source escaped its workspace");
            }
            track.source.path = resolved;
        }
    }
    Ok((q, raw))
}
fn request_directory(request: &Path) -> Result<PathBuf> {
    Ok(request
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?)
}
fn packet_path(request: &Path, output: &Path, existing: bool) -> Result<PathBuf> {
    if output.is_absolute() {
        return Ok(output.to_path_buf());
    }
    if output.as_os_str().is_empty()
        || !output
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        bail!("relative album output must remain beneath the request workspace");
    }
    let base = request_directory(request)?;
    let resolved = base.join(output);
    let checked = if existing {
        resolved.canonicalize()?
    } else {
        resolved
            .parent()
            .context("album output parent missing")?
            .canonicalize()?
    };
    if !checked.starts_with(&base) {
        bail!("relative album output escaped its workspace");
    }
    Ok(resolved)
}
fn validate(q: &Request) -> Result<()> {
    let input_bytes = q
        .tracks
        .iter()
        .try_fold(q.ffmpeg.bytes, |sum, track| {
            sum.checked_add(track.source.bytes)
        })
        .context("album input allowance overflow")?;
    if q.limits.max_input_bytes > 32 * 1024 * 1024 * 1024 || input_bytes > q.limits.max_input_bytes
    {
        bail!("album input byte budget exceeded");
    }
    if q.schema != "reel.music-album-master.v1"
        || !q.permissions.private_only
        || q.permissions.external_upload
        || q.permissions.public_release
        || !label(&q.title)
        || !label(&q.delivery_stem)
        || q.delivery_stem.contains(['/', '\\', '"'])
        || q.tracks.is_empty()
        || q.tracks.len() > 128
        || q.gap_frames > 5 * RATE
        || !q.target_lufs.is_finite()
        || !(-24.0..=-12.0).contains(&q.target_lufs)
        || !q.source_peak_ceiling_dbtp.is_finite()
        || !(-6.0..=-2.0).contains(&q.source_peak_ceiling_dbtp)
        || !(1..=900 * RATE).contains(&q.limits.max_track_frames)
        || !(1..=12 * 3600 * RATE).contains(&q.limits.max_total_frames)
        || !(1..=3600).contains(&q.limits.timeout_seconds)
        || q.limits.max_output_bytes > 32 * 1024 * 1024 * 1024
    {
        bail!("unsupported album scope or limits");
    }
    let mut ids = BTreeSet::new();
    for t in &q.tracks {
        if !label(&t.asset_id)
            || !label(&t.revision)
            || !label(&t.title)
            || !ids.insert(&t.asset_id)
        {
            bail!("album track identity/order invalid");
        }
    }
    // PCM staging + lossless worst case + listening derivative + bounded metadata.
    let estimate = q
        .limits
        .max_total_frames
        .checked_mul(14)
        .context("album estimate overflow")?
        + 2 * 1024 * 1024;
    if estimate > q.limits.max_output_bytes {
        bail!("album output budget below declared worst case");
    }
    Ok(())
}
fn guards(q: &Request) -> Result<Vec<File>> {
    let mut held = vec![hold(&q.ffmpeg)?];
    for track in &q.tracks {
        held.push(hold(&track.source)?);
    }
    Ok(held)
}
pub fn plan(path: &Path) -> Result<Value> {
    let (q, raw) = load(path)?;
    let _held = guards(&q)?;
    Ok(
        json!({"schema":"reel.music-album-plan.v1","request_sha256":sha256_bytes(&raw),
        "tracks":q.tracks.len(),"max_frames":q.limits.max_total_frames,
        "max_output_bytes":q.limits.max_output_bytes,"media_executed":false,"human_approval":false,"published":false}),
    )
}
fn command(q: &Request) -> Command {
    let mut c = Command::new(&q.ffmpeg.path);
    c.args(["-nostdin", "-hide_banner", "-nostats", "-n"]);
    c
}
fn source(c: &mut Command, path: &Path) {
    c.args([
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        "wav,flac,mp3",
        "-i",
    ])
    .arg(path);
}
fn capture(c: &mut Command, maximum: usize, deadline: Instant) -> Result<(Vec<u8>, Vec<u8>)> {
    process::run(c, deadline, move |s| {
        let mut out = Vec::new();
        s.take(maximum as u64 + 1).read_to_end(&mut out)?;
        if out.len() > maximum {
            bail!("album decoder output budget exceeded");
        }
        Ok(out)
    })
}
fn levels(q: &Request, path: &Path, deadline: Instant) -> Result<(f64, f64)> {
    let mut c = command(q);
    c.args(["-v", "info"]);
    source(&mut c, path);
    c.args([
        "-map",
        "0:a:0",
        "-vn",
        "-af",
        "loudnorm=I=-16:TP=-2:LRA=11:print_format=json",
        "-f",
        "null",
        "-",
    ]);
    let (_, stderr) = capture(&mut c, 0, deadline)?;
    let start = stderr
        .iter()
        .rposition(|b| *b == b'{')
        .context("album level report missing")?;
    let end = stderr
        .iter()
        .rposition(|b| *b == b'}')
        .context("album level report incomplete")?;
    let value: Value = serde_json::from_slice(&stderr[start..=end])?;
    let read = |name: &str| -> Result<f64> {
        let n: f64 = value[name]
            .as_str()
            .context("album measurement missing")?
            .parse()?;
        if !n.is_finite() {
            bail!("silent/nonfinite album measurement");
        }
        Ok(n)
    };
    Ok((read("input_i")?, read("input_tp")?))
}
fn gain(lufs: f64, peak: f64, target: f64, ceiling: f64) -> f64 {
    (target - lufs).min(ceiling - peak)
}
fn pcm24(sample: f32, multiplier: f64) -> Result<[u8; 3]> {
    let v = sample as f64 * multiplier;
    if !v.is_finite() || v.abs() >= 1.0 {
        bail!("album nonfinite or clipping PCM");
    }
    let n = (v * 8388608.0).round_ties_even();
    if !(-8388608.0..=8388607.0).contains(&n) {
        bail!("album quantization overflow");
    }
    let bytes = (n as i32).to_le_bytes();
    Ok([bytes[0], bytes[1], bytes[2]])
}
fn metadata(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('=', "\\=")
        .replace(';', "\\;")
        .replace('#', "\\#")
}
fn write_new(path: &Path, raw: &[u8]) -> Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(raw)?;
    Ok(())
}
fn usage(output: &Path, limit: u64) -> Result<()> {
    let mut total = 0u64;
    for entry in fs::read_dir(output)? {
        let entry = entry?;
        let name = entry.file_name();
        let allowed = name.to_str().is_some_and(|name| {
            OUTPUTS.contains(&name) || matches!(name, "receipt.json" | "master.pcm")
        });
        if !entry.file_type()?.is_file() || !allowed {
            bail!("unexpected album output entry");
        }
        total = total
            .checked_add(entry.metadata()?.len())
            .context("album output overflow")?;
    }
    if total > limit {
        bail!("album packet exceeds output budget");
    }
    Ok(())
}
#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Segment {
    asset_id: String,
    revision: String,
    title: String,
    source: Object,
    start_frame: u64,
    frames: u64,
    neutral_decoded_float_sha256: String,
    mastered_pcm24_sha256: String,
    static_gain_db: f64,
    measured_input_lufs: f64,
    measured_input_true_peak_dbtp: f64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    request: Object,
    frames: u64,
    sample_rate: u64,
    channels: u8,
    gap_frames: u64,
    tracks: Vec<Segment>,
    pcm: Object,
    outputs: std::collections::BTreeMap<String, Object>,
    human_approval: bool,
    published: bool,
}
const OUTPUTS: [&str; 5] = [
    "master.flac",
    "master.mp3",
    "chapters.cue",
    "tracklist.txt",
    "chapters.ffmetadata",
];
fn make_pcm(
    q: &Request,
    master: &mut impl Write,
    deadline: Instant,
) -> Result<(Vec<Segment>, Object, u64)> {
    let mut full_hash = Sha256::new();
    let mut cursor = 0u64;
    let mut tracks = Vec::new();
    let silence = vec![0; q.gap_frames as usize * 6];
    for (i, track) in q.tracks.iter().enumerate() {
        let (lufs, peak) = levels(q, &track.source.path, deadline)?;
        let db = gain(lufs, peak, q.target_lufs, q.source_peak_ceiling_dbtp);
        let mut c = command(q);
        c.args(["-v", "error"]);
        source(&mut c, &track.source.path);
        c.args([
            "-map", "0:a:0", "-vn", "-ar", "48000", "-ac", "2", "-f", "f32le", "-",
        ]);
        let (decoded, _) = capture(&mut c, (q.limits.max_track_frames * 8) as usize, deadline)?;
        if decoded.is_empty() || decoded.len() % 8 != 0 {
            bail!("album decoded track extent invalid");
        }
        let frames = decoded.len() as u64 / 8;
        let gap = if i + 1 < q.tracks.len() {
            q.gap_frames
        } else {
            0
        };
        if cursor + frames + gap > q.limits.max_total_frames {
            bail!("album total frame budget exceeded");
        }
        let mut hash = Sha256::new();
        let multiplier = 10f64.powf(db / 20.0);
        let mut packed = Vec::with_capacity(49152);
        for chunk in decoded.chunks(65536) {
            packed.clear();
            for sample in chunk.chunks_exact(4) {
                packed
                    .extend_from_slice(&pcm24(f32::from_le_bytes(sample.try_into()?), multiplier)?);
            }
            master.write_all(&packed)?;
            hash.update(&packed);
            full_hash.update(&packed);
        }
        tracks.push(Segment {
            asset_id: track.asset_id.clone(),
            revision: track.revision.clone(),
            title: track.title.clone(),
            source: Object {
                sha256: track.source.sha256.clone(),
                bytes: track.source.bytes,
            },
            start_frame: cursor,
            frames,
            neutral_decoded_float_sha256: sha256_bytes(&decoded),
            mastered_pcm24_sha256: hex(&hash.finalize()),
            static_gain_db: db,
            measured_input_lufs: lufs,
            measured_input_true_peak_dbtp: peak,
        });
        if gap > 0 {
            master.write_all(&silence)?;
            full_hash.update(&silence);
        }
        cursor += frames + gap;
    }
    Ok((
        tracks,
        Object {
            sha256: hex(&full_hash.finalize()),
            bytes: cursor * 6,
        },
        cursor,
    ))
}
fn metadata_packets(q: &Request, tracks: &[Segment]) -> Vec<(&'static str, String)> {
    let mut chapters = format!(
        ";FFMETADATA1\ntitle={}\nalbum={}\n",
        metadata(&q.title),
        metadata(&q.title)
    );
    let mut cue = format!(
        "TITLE \"{}\"\nFILE \"master.flac\" WAVE\n",
        q.title.replace('"', "'"),
    );
    let mut listing = format!("{}\n", q.title);
    for (i, t) in tracks.iter().enumerate() {
        chapters += &format!(
            "[CHAPTER]\nTIMEBASE=1/48000\nSTART={}\nEND={}\ntitle={}\n",
            t.start_frame,
            t.start_frame + t.frames,
            metadata(&t.title)
        );
        let scaled = t.start_frame * 75;
        let quotient = scaled / RATE;
        let remainder = scaled % RATE;
        let ticks = quotient
            + u64::from(remainder * 2 > RATE || (remainder * 2 == RATE && quotient % 2 == 1));
        let seconds = t.start_frame / RATE;
        cue += &format!(
            "  TRACK {:02} AUDIO\n    TITLE \"{}\"\n    INDEX 01 {:02}:{:02}:{:02}\n",
            i + 1,
            t.title.replace('"', "'"),
            ticks / 4500,
            ticks / 75 % 60,
            ticks % 75
        );
        listing += &format!(
            "{:02}. {:02}:{:02}:{:02}  {}\n",
            i + 1,
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            t.title
        );
    }
    vec![
        ("chapters.ffmetadata", chapters),
        ("chapters.cue", cue),
        ("tracklist.txt", listing),
    ]
}
pub fn render(request: &Path, output: &Path) -> Result<Value> {
    let output = packet_path(request, output, false)?;
    let output = output.as_path();
    let (q, raw) = load(request)?;
    let _held = guards(&q)?;
    if !output.is_absolute() {
        bail!("album output must be an explicit absolute new directory");
    }
    fs::create_dir(output).context("album output must not exist")?;
    let deadline = Instant::now() + Duration::from_secs(q.limits.timeout_seconds);
    let mut master = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join("master.pcm"))?;
    let (tracks, pcm, cursor) = make_pcm(&q, &mut master, deadline)?;
    master.sync_all()?;
    drop(master);
    if object(&output.join("master.pcm"), q.limits.max_total_frames * 6)? != pcm {
        bail!("album PCM extent differs");
    }
    for (name, text) in metadata_packets(&q, &tracks) {
        write_new(&output.join(name), text.as_bytes())?;
    }
    for (name, codec) in [("master.flac", "flac"), ("master.mp3", "libmp3lame")] {
        let mut c = command(&q);
        c.args([
            "-v", "error", "-f", "s24le", "-ar", "48000", "-ac", "2", "-i",
        ])
        .arg(output.join("master.pcm"));
        c.args(["-f", "ffmetadata", "-i"])
            .arg(output.join("chapters.ffmetadata"));
        c.args(["-map_metadata", "1", "-map_chapters", "1", "-c:a", codec]);
        if codec == "flac" {
            c.args(["-compression_level", "8"]);
        } else {
            c.args(["-id3v2_version", "3", "-b:a", "320k"]);
        }
        c.args(["-fs", &q.limits.max_output_bytes.to_string()])
            .arg(output.join(name));
        capture(&mut c, 0, deadline)?;
        usage(output, q.limits.max_output_bytes)?;
    }
    let mut outputs = std::collections::BTreeMap::new();
    for name in OUTPUTS {
        outputs.insert(
            name.into(),
            object(&output.join(name), q.limits.max_output_bytes)?,
        );
    }
    let receipt = Receipt {
        schema: "reel.music-album-receipt.v1".into(),
        request: Object {
            sha256: sha256_bytes(&raw),
            bytes: raw.len() as u64,
        },
        frames: cursor,
        sample_rate: RATE,
        channels: 2,
        gap_frames: q.gap_frames,
        tracks,
        pcm,
        outputs,
        human_approval: false,
        published: false,
    };
    verify_audio(&q, &receipt, output, deadline)?;
    // Original controls remain independently checkable; never overwrite an existing packet.
    write_new(
        &output.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    usage(output, q.limits.max_output_bytes)?;
    let result = check_inner(&q, &raw, output, deadline)?;
    fs::remove_file(output.join("master.pcm"))?;
    Ok(result)
}
fn decoded_identity(
    q: &Request,
    path: &Path,
    float: bool,
    maximum: u64,
    deadline: Instant,
) -> Result<Object> {
    let mut c = command(q);
    c.args(["-v", "error"]);
    source(&mut c, path);
    c.args([
        "-map",
        "0:a:0",
        "-vn",
        "-ar",
        "48000",
        "-ac",
        "2",
        "-f",
        if float { "f32le" } else { "s24le" },
        "-",
    ]);
    let (result, _) = process::run(&mut c, deadline, move |s| identity(s, maximum))?;
    Ok(result)
}
fn verify_audio(q: &Request, r: &Receipt, output: &Path, deadline: Instant) -> Result<()> {
    if decoded_identity(
        q,
        &output.join("master.flac"),
        false,
        r.frames * 6,
        deadline,
    )? != r.pcm
    {
        bail!("lossless album differs from ordered PCM");
    }
    let listening = decoded_identity(
        q,
        &output.join("master.mp3"),
        true,
        (r.frames + RATE / 20) * 8,
        deadline,
    )?;
    if listening.bytes % 8 != 0 || (listening.bytes / 8).abs_diff(r.frames) > RATE / 20 {
        bail!("listening album extent differs");
    }
    let packet_hash = |path: &Path, encode: bool| -> Result<Vec<u8>> {
        let mut c = command(q);
        c.args(["-v", "error"]);
        source(&mut c, path);
        c.args([
            "-map",
            "0:a:0",
            "-vn",
            "-c:a",
            if encode { "libmp3lame" } else { "copy" },
        ]);
        if encode {
            c.args(["-b:a", "320k"]);
        }
        c.args(["-f", "hash", "-hash", "sha256", "-"]);
        let (hash, _) = capture(&mut c, 1024, deadline)?;
        let text = std::str::from_utf8(&hash)?.trim();
        if !text.strip_prefix("SHA256=").is_some_and(digest_valid) {
            bail!("album encoded audio hash unavailable");
        }
        Ok(text.as_bytes().to_vec())
    };
    if packet_hash(&output.join("master.flac"), true)?
        != packet_hash(&output.join("master.mp3"), false)?
    {
        bail!("listening album encoded audio differs from declared PCM and encoder");
    }
    for name in ["master.flac", "master.mp3"] {
        if levels(q, &output.join(name), deadline)?.1 > -1.0 {
            bail!("album final true peak exceeds delivery ceiling");
        }
    }
    Ok(())
}
pub fn check(request: &Path, output: &Path) -> Result<Value> {
    let output = packet_path(request, output, true)?;
    let output = output.as_path();
    let (q, raw) = load(request)?;
    let _held = guards(&q)?;
    check_inner(
        &q,
        &raw,
        output,
        Instant::now() + Duration::from_secs(q.limits.timeout_seconds),
    )
}
fn check_inner(q: &Request, raw: &[u8], output: &Path, deadline: Instant) -> Result<Value> {
    usage(output, q.limits.max_output_bytes)?;
    let mut bytes = Vec::new();
    File::open(output.join("receipt.json"))?
        .take(REQUEST_MAX + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > REQUEST_MAX {
        bail!("album receipt budget");
    }
    crate::approval_attestation::ensure_no_duplicate_keys(&bytes)?;
    let r: Receipt = serde_json::from_slice(&bytes)?;
    if r.schema != "reel.music-album-receipt.v1"
        || r.request
            != (Object {
                sha256: sha256_bytes(raw),
                bytes: raw.len() as u64,
            })
        || r.human_approval
        || r.published
        || r.sample_rate != RATE
        || r.channels != 2
        || r.frames > q.limits.max_total_frames
        || r.gap_frames != q.gap_frames
        || r.tracks.len() != q.tracks.len()
        || r.outputs.len() != OUTPUTS.len()
    {
        bail!("album receipt/source contract differs");
    }
    let mut cursor = 0;
    for (i, (a, b)) in r.tracks.iter().zip(&q.tracks).enumerate() {
        if a.asset_id != b.asset_id
            || a.revision != b.revision
            || a.title != b.title
            || a.source
                != (Object {
                    sha256: b.source.sha256.clone(),
                    bytes: b.source.bytes,
                })
            || a.start_frame != cursor
            || a.frames == 0
            || a.frames > q.limits.max_track_frames
        {
            bail!("album track contract differs");
        }
        cursor += a.frames
            + if i + 1 < r.tracks.len() {
                r.gap_frames
            } else {
                0
            };
    }
    if cursor != r.frames || r.pcm.bytes != r.frames * 6 {
        bail!("album track clock differs");
    }
    for name in OUTPUTS {
        if r.outputs.get(name) != Some(&object(&output.join(name), q.limits.max_output_bytes)?) {
            bail!("album output identity differs");
        }
    }
    usage(output, q.limits.max_output_bytes)?;
    for (name, text) in metadata_packets(q, &r.tracks) {
        if r.outputs.get(name)
            != Some(&Object {
                sha256: sha256_bytes(text.as_bytes()),
                bytes: text.len() as u64,
            })
        {
            bail!("album chapter or tracklist differs from source");
        }
    }
    let (expected_tracks, expected_pcm, expected_frames) =
        make_pcm(q, &mut std::io::sink(), deadline)?;
    if r.tracks != expected_tracks || r.pcm != expected_pcm || r.frames != expected_frames {
        bail!("album differs from independently recomputed source PCM");
    }
    verify_audio(q, &r, output, deadline)?;
    Ok(
        json!({"schema":"reel.music-album-check.v1","tracks":r.tracks.len(),"frames":r.frames,"sample_rate":RATE,
        "lossless_pcm_exact":true,"request_sha256":r.request.sha256,"human_approval":false,"published":false,"production_claim":null}),
    )
}

#[cfg(test)]
mod tests;
