//! Hash-bound presentation layers over an existing film, with unchanged audio packets.
use crate::scene_delivery::{FileRef, checked_file};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, io::Read, path::Path, process::Command};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub language: String,
    pub film: FileRef,
    pub timeline: FileRef,
    pub overlays: Vec<Overlay>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    pub presentation_scene: FileRef,
    pub target_scene: FileRef,
    pub template: FileRef,
    pub template_receipt: FileRef,
    pub layer: FileRef,
    pub font: FileRef,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Timeline {
    schema: String,
    source_film_sha256: String,
    frame_count: u64,
    fps_numerator: u64,
    fps_denominator: u64,
    units: Vec<Unit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Unit {
    scene_id: String,
    first_picture_slot_id: String,
    start_frame: u64,
    frame_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Time {
    numerator: i128,
    denominator: i128,
}

impl Time {
    fn new(n: i128, d: i128) -> Result<Self> {
        if d <= 0 {
            bail!("invalid time base");
        }
        let (mut a, mut b) = (n.abs(), d);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Ok(Self {
            numerator: n / a,
            denominator: d / a,
        })
    }
    fn centiseconds(&self) -> Result<i128> {
        self.numerator
            .checked_mul(100)
            .context("time overflow")
            .map(|n| n.div_euclid(self.denominator))
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Packet {
    pts: Time,
    dts: Time,
    duration: Time,
    bytes: u64,
    payload_hash: String,
    side_data: Value,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Clocks {
    video_pts: Vec<Time>,
    audio_codec: Value,
    audio_packets: Vec<Packet>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
struct Placement {
    scene_id: String,
    picture_slot_id: String,
    start_frame: u64,
    end_frame: u64,
    start_centiseconds: i128,
    end_centiseconds: i128,
    layer_sha256: String,
    font_sha256: String,
}

#[derive(Deserialize, Serialize)]
pub struct Receipt {
    schema: String,
    manifest_sha256: String,
    source_film_sha256: String,
    output_sha256: String,
    output_bytes: u64,
    source_clocks_sha256: String,
    output_clocks_sha256: String,
    frame_count: usize,
    audio_packet_count: usize,
    placements: Vec<Placement>,
    font_configuration_sha256: String,
    font_provider: String,
    render_log_sha256: String,
    font_selection_verified: bool,
    presentation_pixels_verified: bool,
    publication: String,
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn file_hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn command(name: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(name);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn output(cmd: &mut Command) -> Result<Vec<u8>> {
    let result = cmd.output()?;
    if !result.status.success() {
        bail!(
            "media command failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(result.stdout)
}

fn read(root: &Path, item: &FileRef) -> Result<Vec<u8>> {
    Ok(fs::read(checked_file(root, item)?)?)
}

fn json(root: &Path, item: &FileRef) -> Result<Value> {
    Ok(serde_json::from_slice(&read(root, item)?)?)
}

fn number(v: &Value) -> Result<i128> {
    if let Some(n) = v.as_i64() {
        return Ok(i128::from(n));
    }
    Ok(v.as_str().context("missing timestamp")?.parse()?)
}

fn clocks(path: &Path) -> Result<Clocks> {
    let streams: Value = serde_json::from_slice(&output(
        command("ffprobe")
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_data_hash",
                "sha256",
                "-of",
                "json",
            ])
            .arg(path),
    )?)?;
    let streams = streams["streams"].as_array().context("no streams")?;
    let videos = streams
        .iter()
        .filter(|s| s["codec_type"] == "video")
        .collect::<Vec<_>>();
    let audios = streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .collect::<Vec<_>>();
    if videos.len() != 1 || audios.len() != 1 {
        bail!("exactly one video and one audio stream required");
    }
    let timebase = |s: &Value| -> Result<(i128, i128)> {
        let (n, d) = s["time_base"]
            .as_str()
            .context("missing timebase")?
            .split_once('/')
            .context("invalid timebase")?;
        Ok((n.parse()?, d.parse()?))
    };
    let (vn, vd) = timebase(videos[0])?;
    let frames: Value = serde_json::from_slice(&output(
        command("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_frames",
                "-show_entries",
                "frame=best_effort_timestamp",
                "-of",
                "json",
            ])
            .arg(path),
    )?)?;
    let video_pts = frames["frames"]
        .as_array()
        .context("missing frames")?
        .iter()
        .map(|f| Time::new(number(&f["best_effort_timestamp"])? * vn, vd))
        .collect::<Result<Vec<_>>>()?;
    let (an, ad) = timebase(audios[0])?;
    let packets: Value = serde_json::from_slice(&output(
        command("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_packets",
                "-show_data_hash",
                "sha256",
                "-show_entries",
                "packet=pts,dts,duration,size,data_hash,side_data_list",
                "-of",
                "json",
            ])
            .arg(path),
    )?)?;
    let audio_packets = packets["packets"]
        .as_array()
        .context("missing packets")?
        .iter()
        .map(|p| {
            Ok(Packet {
                pts: Time::new(number(&p["pts"])? * an, ad)?,
                dts: Time::new(number(&p["dts"])? * an, ad)?,
                duration: Time::new(number(&p["duration"])? * an, ad)?,
                bytes: number(&p["size"])?.try_into()?,
                payload_hash: p["data_hash"]
                    .as_str()
                    .context("missing packet hash")?
                    .into(),
                side_data: p["side_data_list"].clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if video_pts.is_empty() || audio_packets.is_empty() {
        bail!("empty film stream");
    }
    let audio_codec = serde_json::json!({"codec":audios[0]["codec_name"],"sample_rate":audios[0]["sample_rate"],"channels":audios[0]["channels"],"channel_layout":audios[0]["channel_layout"],"profile":audios[0]["profile"],"extradata_hash":audios[0]["extradata_hash"],"side_data":audios[0]["side_data_list"]});
    Ok(Clocks {
        video_pts,
        audio_codec,
        audio_packets,
    })
}

fn ass_time(cs: i128) -> Result<String> {
    if cs < 0 {
        bail!("negative ASS time");
    }
    Ok(format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360000,
        (cs / 6000) % 60,
        (cs / 100) % 60,
        cs % 100
    ))
}

fn inputs(manifest: &Manifest, root: &Path, clock: &Clocks) -> Result<Vec<(Placement, String)>> {
    if manifest.schema != "reel.film-presentation-overlay.v1"
        || manifest.language.is_empty()
        || manifest.overlays.is_empty()
    {
        bail!("invalid film overlay manifest");
    }
    checked_file(root, &manifest.film)?;
    let timeline: Timeline = serde_json::from_slice(&read(root, &manifest.timeline)?)?;
    if timeline.schema != "reel.source-film-timeline.v1"
        || timeline.source_film_sha256 != manifest.film.sha256
        || timeline.frame_count != clock.video_pts.len() as u64
        || timeline.fps_numerator == 0
        || timeline.fps_denominator == 0
    {
        bail!("film timeline mismatch");
    }
    let origin = &clock.video_pts[0];
    for (i, pts) in clock.video_pts.iter().enumerate() {
        let expected = Time::new(
            origin.numerator * i128::from(timeline.fps_numerator)
                + i as i128 * i128::from(timeline.fps_denominator) * origin.denominator,
            origin.denominator * i128::from(timeline.fps_numerator),
        )?;
        if pts != &expected {
            bail!("source picture clock is not continuous at frame {i}");
        }
    }
    let mut ids = BTreeSet::new();
    let mut previous_end = 0;
    for unit in &timeline.units {
        if !ids.insert(&unit.scene_id)
            || unit.frame_count == 0
            || unit.start_frame < previous_end
            || unit
                .start_frame
                .checked_add(unit.frame_count)
                .context("unit overflow")?
                > timeline.frame_count
        {
            bail!("invalid/duplicate timeline unit");
        }
        previous_end = unit.start_frame + unit.frame_count;
    }
    let mut result = Vec::new();
    let mut targets = BTreeSet::new();
    for overlay in &manifest.overlays {
        let presentation = json(root, &overlay.presentation_scene)?;
        let target = json(root, &overlay.target_scene)?;
        let template = json(root, &overlay.template)?;
        let template_receipt = json(root, &overlay.template_receipt)?;
        let rate = template_receipt["sample_rate"]
            .as_u64()
            .context("missing template sample rate")?;
        if rate == 0
            || template_receipt["schema"] != "reel.editable-layer-compile-receipt.v1"
            || template_receipt["scene_id"] != presentation["scene_id"]
            || template_receipt["language"] != manifest.language
            || template_receipt["template_id"] != template["template_id"]
            || template_receipt["template_definition_sha256"] != overlay.template.sha256
            || template_receipt["ass_sha256"] != overlay.layer.sha256
            || template_receipt["ass_bytes"].as_u64() != Some(overlay.layer.bytes)
            || template_receipt["duration_samples"].as_u64()
                != Some(rate.checked_mul(4).context("template duration overflow")?)
        {
            bail!("editable chapter template receipt mismatch");
        }
        let scene_id = target["scene_id"]
            .as_str()
            .context("missing target scene id")?;
        if !targets.insert(scene_id.to_owned()) {
            bail!("duplicate scene overlay");
        }
        let unit = timeline
            .units
            .iter()
            .find(|u| u.scene_id == scene_id)
            .context("target absent from film timeline")?;
        let placement = &presentation["presentation"]["placement"];
        let first = target["language_event_bindings"][&manifest.language]
            .as_array()
            .and_then(|v| v.first())
            .context("missing target first picture")?;
        if placement["mode"] != "overlay-on-target-scene-start"
            || placement["target_scene_id"] != scene_id
            || placement["target_picture_slot_id"] != unit.first_picture_slot_id
            || first["picture_slot_id"] != unit.first_picture_slot_id
            || presentation["presentation"]["template_id"] != template["template_id"]
            || template["kind"] != "chapter-title"
            || template["fixed_duration_seconds"].as_u64() != Some(4)
        {
            bail!("chapter placement/template contract mismatch");
        }
        let duration = timeline
            .fps_numerator
            .checked_mul(4)
            .context("duration overflow")?;
        if duration % timeline.fps_denominator != 0 {
            bail!("four-second chapter duration is not frame-exact");
        }
        let count = duration / timeline.fps_denominator;
        if count > unit.frame_count {
            bail!("chapter overlay exceeds target scene");
        }
        let start: usize = unit.start_frame.try_into()?;
        let end: usize = unit
            .start_frame
            .checked_add(count)
            .context("end overflow")?
            .try_into()?;
        let boundary = if let Some(pts) = clock.video_pts.get(end) {
            pts.clone()
        } else {
            Time::new(
                origin.numerator * i128::from(timeline.fps_numerator)
                    + end as i128 * i128::from(timeline.fps_denominator) * origin.denominator,
                origin.denominator * i128::from(timeline.fps_numerator),
            )?
        };
        let s = clock.video_pts[start].centiseconds()?;
        let e = boundary.centiseconds()?;
        if start > 0 && clock.video_pts[start - 1].centiseconds()? >= s
            || clock.video_pts[end - 1].centiseconds()? >= e
        {
            bail!("ASS centisecond precision cannot express frame boundary");
        }
        let ass = String::from_utf8(read(root, &overlay.layer)?)?;
        let mut lines = Vec::new();
        let mut dialogue_count = 0;
        for line in ass.lines() {
            if let Some(rest) = line.strip_prefix("Dialogue:") {
                let mut fields = rest.splitn(10, ',').map(str::to_owned).collect::<Vec<_>>();
                if fields.len() != 10
                    || fields[1].trim() != "0:00:00.00"
                    || fields[2].trim() != "0:00:04.00"
                {
                    bail!("chapter ASS must contain only local 0-4 second dialogue");
                }
                fields[1] = ass_time(s)?;
                fields[2] = ass_time(e)?;
                lines.push(format!("Dialogue:{}", fields.join(",")));
                dialogue_count += 1;
            } else {
                lines.push(line.to_owned());
            }
        }
        if dialogue_count == 0 {
            bail!("empty chapter ASS");
        }
        checked_file(root, &overlay.font)?;
        result.push((
            Placement {
                scene_id: scene_id.to_owned(),
                picture_slot_id: unit.first_picture_slot_id.clone(),
                start_frame: unit.start_frame,
                end_frame: end as u64,
                start_centiseconds: s,
                end_centiseconds: e,
                layer_sha256: overlay.layer.sha256.clone(),
                font_sha256: overlay.font.sha256.clone(),
            },
            lines.join("\n") + "\n",
        ));
    }
    Ok(result)
}

fn decode(path: &Path) -> Result<()> {
    output(
        command("ffmpeg")
            .args(["-v", "error", "-xerror", "-i"])
            .arg(path)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"]),
    )?;
    Ok(())
}

pub fn build(manifest_path: &Path, root: &Path, out: &Path) -> Result<Receipt> {
    if out.exists() {
        bail!("output directory already exists");
    }
    let bytes = fs::read(manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let film = checked_file(root, &manifest.film)?;
    let source = clocks(&film)?;
    let layers = inputs(&manifest, root, &source)?;
    fs::create_dir_all(out.join("fonts"))?;
    let out = fs::canonicalize(out)?;
    let mut filters = Vec::new();
    for (i, ((_, ass), overlay)) in layers.iter().zip(&manifest.overlays).enumerate() {
        fs::write(out.join(format!("layer-{i}.ass")), ass)?;
        fs::write(
            out.join("fonts")
                .join(format!("{}.ttf", overlay.font.sha256)),
            read(root, &overlay.font)?,
        )?;
        filters.push(format!("ass=layer-{i}.ass:fontsdir=fonts"));
    }
    let movie = out.join("review.mp4");
    let font_dir = out
        .join("fonts")
        .to_string_lossy()
        .replace('\\', "/")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let font_config = format!(
        "<?xml version=\"1.0\"?><!DOCTYPE fontconfig SYSTEM \"fonts.dtd\"><fontconfig><dir>{font_dir}</dir></fontconfig>\n"
    );
    fs::write(out.join("fonts.conf"), &font_config)?;
    let rendered_command = command("ffmpeg")
        .current_dir(&out)
        .env("FONTCONFIG_FILE", out.join("fonts.conf"))
        .args(["-v", "verbose", "-copyts", "-i"])
        .arg(&film)
        .args(["-map", "0:v:0", "-map", "0:a:0", "-vf"])
        .arg(filters.join(","))
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "16",
            "-pix_fmt",
            "yuv420p",
            "-fps_mode:v",
            "passthrough",
            "-enc_time_base:v",
            "demux",
            "-c:a",
            "copy",
            "-avoid_negative_ts",
            "disabled",
        ])
        .arg(&movie)
        .output()?;
    fs::write(out.join("render.log"), &rendered_command.stderr)?;
    if !rendered_command.status.success() {
        bail!(
            "film overlay render failed: {}",
            String::from_utf8_lossy(&rendered_command.stderr)
        );
    }
    let log = String::from_utf8_lossy(&rendered_command.stderr);
    let font_provider = log
        .lines()
        .find_map(|line| {
            line.split_once("Using font provider ")
                .map(|(_, provider)| provider.trim().to_owned())
        })
        .context("font provider not reported by renderer")?;
    let rendered = clocks(&movie)?;
    if rendered != source {
        bail!("film picture PTS or compressed audio packet/timestamp closure mismatch");
    }
    decode(&movie)?;
    let receipt = Receipt {
        schema: "reel.film-presentation-overlay-receipt.v1".into(),
        manifest_sha256: hash(&bytes),
        source_film_sha256: manifest.film.sha256,
        output_sha256: file_hash(&movie)?,
        output_bytes: fs::metadata(&movie)?.len(),
        source_clocks_sha256: hash(&serde_json::to_vec(&source)?),
        output_clocks_sha256: hash(&serde_json::to_vec(&rendered)?),
        frame_count: source.video_pts.len(),
        audio_packet_count: source.audio_packets.len(),
        placements: layers.into_iter().map(|(p, _)| p).collect(),
        font_configuration_sha256: hash(font_config.as_bytes()),
        font_provider,
        render_log_sha256: hash(&rendered_command.stderr),
        font_selection_verified: false,
        presentation_pixels_verified: false,
        publication: "not-authorized".into(),
    };
    fs::write(
        out.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(receipt)
}

pub fn check(manifest_path: &Path, root: &Path, out: &Path) -> Result<()> {
    let bytes = fs::read(manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let source = clocks(&checked_file(root, &manifest.film)?)?;
    let layers = inputs(&manifest, root, &source)?;
    for (i, (_, ass)) in layers.iter().enumerate() {
        if fs::read(out.join(format!("layer-{i}.ass")))? != ass.as_bytes() {
            bail!("retained presentation layer mismatch");
        }
    }
    let placements = layers.into_iter().map(|(p, _)| p).collect::<Vec<_>>();
    let receipt: Receipt = serde_json::from_slice(&fs::read(out.join("receipt.json"))?)?;
    let movie = out.join("review.mp4");
    let rendered = clocks(&movie)?;
    let log = fs::read(out.join("render.log"))?;
    let log_text = String::from_utf8_lossy(&log);
    let provider = log_text
        .lines()
        .find_map(|line| {
            line.split_once("Using font provider ")
                .map(|(_, provider)| provider.trim())
        })
        .context("missing recorded font provider")?;
    for overlay in &manifest.overlays {
        let selected = read(root, &overlay.font)?;
        if fs::read(
            out.join("fonts")
                .join(format!("{}.ttf", overlay.font.sha256)),
        )? != selected
        {
            bail!("retained render font mismatch");
        }
    }
    if receipt.schema != "reel.film-presentation-overlay-receipt.v1"
        || receipt.manifest_sha256 != hash(&bytes)
        || receipt.source_film_sha256 != manifest.film.sha256
        || receipt.output_sha256 != file_hash(&movie)?
        || receipt.output_bytes != fs::metadata(&movie)?.len()
        || receipt.placements != placements
        || receipt.frame_count != source.video_pts.len()
        || receipt.audio_packet_count != source.audio_packets.len()
        || rendered != source
        || receipt.source_clocks_sha256 != hash(&serde_json::to_vec(&source)?)
        || receipt.output_clocks_sha256 != hash(&serde_json::to_vec(&rendered)?)
        || receipt.font_configuration_sha256 != file_hash(&out.join("fonts.conf"))?
        || receipt.presentation_pixels_verified
        || receipt.font_selection_verified
        || receipt.render_log_sha256 != hash(&log)
        || receipt.font_provider != provider
    {
        bail!("film overlay receipt/output closure mismatch");
    }
    decode(&movie)
}
