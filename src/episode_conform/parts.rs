//! Portable, bounded programme production. V1 monolithic delivery is unchanged.
//! Proof consumption rehashes compact/PCM inputs, not unseen retained lossless media.
use super::*;
use std::io::{Read, Write};

const PROOF: &str = "reel.programme-part-proof.v2";
pub const RECEIPT: &str = "reel.episode-conform-receipt.v2";
const PROFILE: &str = "h264-independent-crf-yuv420p-bf0-mp3-continuous.v1";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub start: usize,
    pub end: usize,
    pub start_frame: u64,
    pub start_sample: u64,
}
impl Range {
    pub(super) fn contains(&self, index: usize) -> bool {
        (self.start..self.end).contains(&index)
    }
    pub(super) fn validate(&self, manifest: &Manifest) -> Result<()> {
        if self.start >= self.end
            || self.end > manifest.segments.len()
            // Three retained media outputs per segment plus one proof must fit
            // the unchanged asset-job limit of 64 declared outputs.
            || self.end - self.start > 21
            || manifest.preassembled_master.is_some()
            || manifest.audio_frame_conform.as_deref() != Some("pad-silence-to-picture-boundaries")
            || manifest.output_frame_rate.is_none()
            || (self.start == 0 && (self.start_frame != 0 || self.start_sample != 0))
        {
            bail!("invalid bounded programme range or clock policy");
        }
        let profile = compact_profile(manifest)?;
        if !profile.retain_lossless_master
            || ![
                32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
            ]
            .contains(&profile.audio_bitrate_kbps)
            || ![44100, 48000].contains(&manifest.output_sample_rate)
        {
            bail!("bounded programme requires retained lossless parts and an MP3 profile");
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedPart {
    pub file: scene_delivery::FileRef,
    pub cache_uri: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub index: usize,
    pub native: SegmentReceipt,
    pub lossless: RetainedPart,
    pub video: scene_delivery::FileRef,
    pub pcm: scene_delivery::FileRef,
    pub compact_yuv420_sha256: String,
    pub video_signature: serde_json::Value,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub schema: String,
    pub manifest_sha256: String,
    pub episode_id: String,
    pub language: String,
    pub profile: String,
    pub crf: u8,
    pub audio_bitrate_kbps: u16,
    pub validator_sha256: String,
    pub ffmpeg_sha256: String,
    pub ffprobe_sha256: String,
    pub producer_job_sha256: String,
    pub source_scope: Vec<scene_delivery::FileRef>,
    pub range: Range,
    pub(crate) facts: MediaFacts,
    pub parts: Vec<Part>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartSet {
    pub schema: String,
    pub manifest: scene_delivery::FileRef,
    /// Immutable proof FileRefs are the trust roots, like native scene proofs.
    pub proofs: Vec<scene_delivery::FileRef>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub episode_id: String,
    pub language: String,
    pub manifest_sha256: String,
    pub output_sample_rate: u32,
    pub total_frames: u64,
    pub total_samples: u64,
    pub timestamps_verified: bool,
    pub compact_ordered_content_verified: bool,
    pub preencode_pcm_identity_verified: bool,
    pub lossless_master: LosslessMaster,
    pub delivery: scene_delivery::FileRef,
    pub segments: Vec<SegmentReceipt>,
    pub consumed_proofs: Vec<scene_delivery::FileRef>,
    pub finalizer_job_sha256: String,
    pub boundary_findings: Vec<episode_delivery::BoundaryFinding>,
    pub limitations: Vec<String>,
    pub publication: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "representation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LosslessMaster {
    OrderedImmutableParts {
        parts: Vec<RetainedPart>,
        custody: String,
    },
}

fn reference(root: &Path, name: &str) -> Result<scene_delivery::FileRef> {
    let path = root.join(name);
    Ok(scene_delivery::FileRef {
        path: name.into(),
        sha256: file_sha(&path)?,
        bytes: fs::metadata(path)?.len(),
    })
}
fn run(mut cmd: Command) -> Result<()> {
    if !cmd.status()?.success() {
        bail!("bounded programme media operation failed");
    }
    Ok(())
}
fn video_signature(path: &Path) -> Result<serde_json::Value> {
    let out = command("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_streams",
            "-show_data_hash",
            "sha256",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !out.status.success() {
        bail!("compact video probe failed");
    }
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let stream = &value["streams"][0];
    if stream["codec_name"] != "h264"
        || stream["pix_fmt"] != "yuv420p"
        || stream["has_b_frames"] != 0
        || stream["extradata_hash"].as_str().is_none()
    {
        bail!("unsupported compact video codec/reordering/extradata");
    }
    let mut signature = serde_json::Map::new();
    for key in [
        "codec_name",
        "profile",
        "level",
        "width",
        "height",
        "pix_fmt",
        "sample_aspect_ratio",
        "r_frame_rate",
        "time_base",
        "color_range",
        "color_space",
        "color_transfer",
        "color_primaries",
        "extradata_hash",
    ] {
        signature.insert(key.into(), stream[key].clone());
    }
    Ok(signature.into())
}
/// Full decoded frame clock and packet DTS/keyframe checks. BF=0 is deliberate.
fn video_clock(path: &Path, frames: u64, n: u64, d: u64) -> Result<()> {
    let out = command("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_packets",
            "-show_entries",
            "packet=pts_time,dts_time,flags",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !out.status.success() {
        bail!("video packet clock probe failed");
    }
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let packets = value["packets"]
        .as_array()
        .context("video packets missing")?;
    if packets.len() as u64 != frames
        || !packets[0]["flags"]
            .as_str()
            .unwrap_or_default()
            .contains('K')
    {
        bail!("video packet coverage or random access missing");
    }
    for (i, packet) in packets.iter().enumerate() {
        let expected = i as f64 * d as f64 / n as f64;
        for key in ["pts_time", "dts_time"] {
            let actual: f64 = packet[key]
                .as_str()
                .context("video clock missing")?
                .parse()?;
            if !actual.is_finite() || (actual - expected).abs() > 0.000_002 {
                bail!("video PTS/DTS discontinuity");
            }
        }
    }
    Ok(())
}

/// Normalize a contiguous range using the v1 source/geometry/repeat/hold engine.
/// Offsets are explicit controls; finalization rejects any false offset/coverage.
pub fn produce(manifest: &Path, assets: &Path, range: &Range, output: &Path) -> Result<()> {
    let scope = checkpoint::snapshot_job_scope(assets)?;
    declared_control(manifest, assets, &scope)?;
    build_internal(
        manifest,
        assets,
        assets,
        output,
        Some(scope),
        None,
        Some(range),
    )?;
    Ok(())
}

/// File-based CLI boundary: range controls must also belong to the closed job.
pub fn produce_from_file(
    manifest: &Path,
    assets: &Path,
    range: &Path,
    output: &Path,
) -> Result<()> {
    let scope = checkpoint::snapshot_job_scope(assets)?;
    declared_control(range, assets, &scope)?;
    if fs::metadata(range)?.len() > 4096 {
        bail!("programme range control too large");
    }
    let range: Range = serde_json::from_slice(&fs::read(range)?)?;
    produce(manifest, assets, &range, output)
}

fn declared_control(path: &Path, root: &Path, scope: &[scene_delivery::FileRef]) -> Result<()> {
    let absolute = path.canonicalize()?;
    let relative = absolute
        .strip_prefix(root.canonicalize()?)
        .context("control outside closed job")?;
    if !scope.iter().any(|f| f.path == relative) {
        bail!("undeclared programme control");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn publish(
    range: &Range,
    manifest_bytes: &[u8],
    facts: &MediaFacts,
    records: &[SegmentReceipt],
    normalized: &[PathBuf],
    work: &Path,
    assets: &Path,
    scope: &[scene_delivery::FileRef],
) -> Result<()> {
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)?;
    let profile = compact_profile(&manifest)?;
    // The shared engine's common geometry facts can retain the *input* sample
    // rate when a bounded range begins with a resampled source. Portable facts
    // describe the actual normalized output, never that input clock.
    let normalized_facts = probe(normalized.first().context("empty normalized range")?)?;
    if (
        normalized_facts.width,
        normalized_facts.height,
        &normalized_facts.fps,
    ) != (facts.width, facts.height, &facts.fps)
        || normalized_facts.sample_rate != manifest.output_sample_rate
    {
        bail!("normalized part output facts differ from programme");
    }
    let facts = &normalized_facts;
    let (n, d) = fps_parts(&facts.fps)?;
    let mut parts = Vec::new();
    for (offset, (native, lossless)) in records.iter().zip(normalized).enumerate() {
        let index = range.start + offset;
        // These are freshly checked in this bounded producer; finalizers consume
        // the resulting immutable proof without opening the lossless cache object.
        let picture = decoded_digest(lossless, true)?;
        let audio = decoded_digest(lossless, false)?;
        if picture.0 != native.decoded_picture_sha256
            || picture.1 != native.frames * facts.width * facts.height * 3
            || audio.0 != native.decoded_audio_sha256
            || audio.1 != native.samples * 6
        {
            bail!("normalized part changed before proof issuance");
        }
        episode_delivery::verify_timestamps(lossless, manifest.output_sample_rate, n, d)?;
        let lossless_name = format!("lossless-{index:05}.mkv");
        fs::copy(lossless, work.join(&lossless_name))?;
        let video_name = format!("video-{index:05}.mp4");
        let pcm_name = format!("audio-{index:05}.pcm");
        let mut cmd = command("ffmpeg");
        cmd.args(["-v", "error", "-nostdin", "-i"])
            .arg(lossless)
            .args(["-map", "0:v:0", "-an", "-c:v", "libx264", "-crf"])
            .arg(profile.crf.to_string())
            .args(["-pix_fmt", "yuv420p", "-bf", "0", "-video_track_timescale"])
            .arg(n.to_string());
        for (key, flag) in [
            ("color_range", "-color_range"),
            ("color_space", "-colorspace"),
            ("color_transfer", "-color_trc"),
            ("color_primaries", "-color_primaries"),
        ] {
            if facts.color_properties[key] != "unknown" {
                cmd.args([flag, &facts.color_properties[key]]);
            }
        }
        match facts.color_properties["color_range"].as_str() {
            "tv" => {
                cmd.args(["-bsf:v", "h264_metadata=video_full_range_flag=0"]);
            }
            "pc" => {
                cmd.args(["-bsf:v", "h264_metadata=video_full_range_flag=1"]);
            }
            _ => {}
        }
        cmd.arg(work.join(&video_name));
        run(cmd)?;
        let mut cmd = command("ffmpeg");
        cmd.args(["-v", "error", "-nostdin", "-i"])
            .arg(lossless)
            .args(["-map", "0:a:0", "-vn", "-c:a", "pcm_s24le", "-f", "s24le"])
            .arg(work.join(&pcm_name));
        run(cmd)?;
        let pcm = reference(work, &pcm_name)?;
        if pcm.sha256 != native.decoded_audio_sha256 || pcm.bytes != native.samples * 6 {
            bail!("exported PCM differs from exact lossless audio");
        }
        let compact = crate::decoded_video_hash::hash(
            &work.join(&video_name),
            "yuv420p",
            None,
            Some(&[native.frames]),
        )?;
        if compact.bytes != native.frames * facts.width * facts.height * 3 / 2 {
            bail!("compact video count mismatch");
        }
        video_clock(&work.join(&video_name), native.frames, n, d)?;
        let file = reference(work, &lossless_name)?;
        parts.push(Part {
            index,
            native: serde_json::from_value(serde_json::to_value(native)?)?,
            lossless: RetainedPart {
                cache_uri: format!("cache://sha256/{}", file.sha256),
                file,
            },
            video: reference(work, &video_name)?,
            pcm,
            compact_yuv420_sha256: compact.values[0].clone(),
            video_signature: video_signature(&work.join(&video_name))?,
        });
    }
    for source in scope {
        scene_delivery::checked_file(assets, source)?;
    }
    let proof = Proof {
        schema: PROOF.into(),
        manifest_sha256: sha(manifest_bytes),
        episode_id: manifest.episode_id.clone(),
        language: manifest.language.clone(),
        profile: PROFILE.into(),
        crf: profile.crf,
        audio_bitrate_kbps: profile.audio_bitrate_kbps,
        validator_sha256: file_sha(&std::env::current_exe()?)?,
        ffmpeg_sha256: checkpoint::tool_hash("ffmpeg")?,
        ffprobe_sha256: checkpoint::tool_hash("ffprobe")?,
        producer_job_sha256: file_sha(&assets.join("job.json"))?,
        source_scope: scope.to_vec(),
        range: serde_json::from_value(serde_json::to_value(range)?)?,
        facts: facts.clone(),
        parts,
    };
    fs::write(
        work.join("part-proof.json"),
        serde_json::to_vec_pretty(&proof)?,
    )?;
    Ok(())
}

fn checked_proofs(set: &PartSet, root: &Path, manifest_bytes: &[u8]) -> Result<Vec<Proof>> {
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)?;
    let profile = compact_profile(&manifest)?;
    if set.schema != "reel.programme-part-set.v2"
        || set.proofs.is_empty()
        || set.proofs.len() > 65536
    {
        bail!("invalid part set");
    }
    let validator = file_sha(&std::env::current_exe()?)?;
    let ffmpeg = checkpoint::tool_hash("ffmpeg")?;
    let ffprobe = checkpoint::tool_hash("ffprobe")?;
    let (mut index, mut frames, mut samples) = (0, 0u64, 0u64);
    let mut signature = None;
    let mut proofs = Vec::new();
    for item in &set.proofs {
        if item.bytes > 4 * 1024 * 1024 {
            bail!("portable part proof too large");
        }
        let proof: Proof =
            serde_json::from_slice(&fs::read(scene_delivery::checked_file(root, item)?)?)?;
        proof.range.validate(&manifest)?;
        if proof.schema != PROOF
            || proof.manifest_sha256 != sha(manifest_bytes)
            || proof.episode_id != manifest.episode_id
            || proof.language != manifest.language
            || proof.profile != PROFILE
            || proof.crf != profile.crf
            || proof.audio_bitrate_kbps != profile.audio_bitrate_kbps
            || proof.validator_sha256 != validator
            || proof.ffmpeg_sha256 != ffmpeg
            || proof.ffprobe_sha256 != ffprobe
            || !valid_sha(&proof.producer_job_sha256)
            || proof.source_scope.is_empty()
            || proof.range.start != index
            || proof.range.start_frame != frames
            || proof.range.start_sample != samples
            || proof.parts.len() != proof.range.end - proof.range.start
        {
            bail!("portable part identity/tool/order/range mismatch");
        }
        let (n, d) = manifest
            .output_frame_rate
            .as_ref()
            .context("output clock missing")?
            .clock()?;
        if fps_parts(&proof.facts.fps)? != (n, d)
            || proof.facts.sample_rate != manifest.output_sample_rate
            || proof.facts.width == 0
            || proof.facts.height == 0
            || proof.facts.width > 16384
            || proof.facts.height > 16384
            || proof.facts.width % 2 != 0
            || proof.facts.height % 2 != 0
            || manifest.output_geometry.as_ref().is_some_and(|g| {
                u64::from(g.width) != proof.facts.width || u64::from(g.height) != proof.facts.height
            })
        {
            bail!("part clock differs from programme");
        }
        for part in &proof.parts {
            let selected = &manifest.segments[index];
            let native = &part.native;
            let compact_bytes = native
                .frames
                .checked_mul(proof.facts.width)
                .and_then(|v| v.checked_mul(proof.facts.height))
                .and_then(|v| v.checked_mul(3))
                .context("compact picture byte overflow")?
                / 2;
            let rate = manifest
                .output_frame_rate
                .as_ref()
                .context("part output rate missing")?;
            let input_fps = native
                .frame_rate_verification
                .as_ref()
                .map_or(proof.facts.fps.as_str(), |v| v.input_fps.as_str());
            let content_frames = rate.frames(native.input_frames, input_fps)?;
            let hold_start = frames
                .checked_add(content_frames)
                .context("hold start overflow")?;
            let expected_hold_samples = hold_samples(
                selected.tail_hold_frames,
                hold_start,
                manifest.output_sample_rate,
                n,
                d,
            )?;
            if part.index != index
                || native.id != selected.id
                || native.kind
                    != if selected.kind == SegmentKind::Scene {
                        "scene"
                    } else {
                        "episode-presentation"
                    }
                || native.selected_master_sha256 != selected.master.sha256
                || native.selected_master_bytes != selected.master.bytes
                || native.source_receipt_sha256 != selected.source_receipt.sha256
                || native.tail_hold_frames != selected.tail_hold_frames
                || native.start_frame != frames
                || native.start_sample != samples
                || native.output_sample_rate != manifest.output_sample_rate
                || native.frames == 0
                || native.samples == 0
                || native.input_frames == 0
                || native.input_samples == 0
                || native.input_sample_rate == 0
                || content_frames.checked_add(selected.tail_hold_frames) != Some(native.frames)
                || native.tail_hold_samples != expected_hold_samples
                || native.frame_rate_verification.as_ref().is_some_and(|v| {
                    v.input_frames != native.input_frames
                        || v.output_frames != content_frames
                        || v.repeated_frames != content_frames.saturating_sub(native.input_frames)
                        || !v.transformed_source_matches_selected
                        || fps_parts(&v.output_fps).ok() != Some((n, d))
                })
                || part.pcm.sha256 != native.decoded_audio_sha256
                || part.pcm.bytes != native.samples.checked_mul(6).context("sample overflow")?
                || part.lossless.cache_uri
                    != format!("cache://sha256/{}", part.lossless.file.sha256)
                || !valid_sha(&part.lossless.file.sha256)
                || part.lossless.file.bytes == 0
            {
                bail!("part source/clock/hold/custody mismatch");
            }
            if !proof
                .source_scope
                .iter()
                .any(|f| f.sha256 == selected.master.sha256 && f.bytes == selected.master.bytes)
                || !proof.source_scope.iter().any(|f| {
                    f.sha256 == selected.source_receipt.sha256
                        && f.bytes == selected.source_receipt.bytes
                })
                || !proof.source_scope.iter().any(|f| {
                    f.path == Path::new("job.json") && f.sha256 == proof.producer_job_sha256
                })
            {
                bail!("part producer scope does not contain selected sources/job");
            }
            if signature
                .as_ref()
                .is_some_and(|s| *s != part.video_signature)
            {
                bail!("incompatible independent video parts");
            }
            signature = Some(part.video_signature.clone());
            let video = scene_delivery::checked_file(root, &part.video)?;
            let pcm = scene_delivery::checked_file(root, &part.pcm)?;
            if expected_hold_samples != 0 {
                use std::io::{Seek, SeekFrom};
                let mut file = fs::File::open(pcm)?;
                let tail = expected_hold_samples
                    .checked_mul(6)
                    .context("hold PCM overflow")?;
                file.seek(SeekFrom::End(-i64::try_from(tail)?))?;
                let mut buffer = [0; 65536];
                loop {
                    let count = file.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    if buffer[..count].iter().any(|b| *b != 0) {
                        bail!("silent hold PCM contains nonzero samples");
                    }
                }
            }
            if part.video_signature["width"].as_u64() != Some(proof.facts.width)
                || part.video_signature["height"].as_u64() != Some(proof.facts.height)
            {
                bail!("compact part geometry differs from proof");
            }
            if video_signature(&video)? != part.video_signature {
                bail!("video signature changed");
            }
            video_clock(&video, native.frames, n, d)?;
            let actual =
                crate::decoded_video_hash::hash(&video, "yuv420p", None, Some(&[native.frames]))?;
            if actual.values != [part.compact_yuv420_sha256.clone()]
                || actual.bytes != compact_bytes
            {
                bail!("compact content changed");
            }
            frames = frames
                .checked_add(native.frames)
                .context("frame overflow")?;
            samples = samples
                .checked_add(native.samples)
                .context("sample overflow")?;
            // Per-range offsets must agree with the exact global rational clock.
            let boundary =
                (u128::from(frames) * u128::from(manifest.output_sample_rate) * u128::from(d)
                    + u128::from(n) / 2)
                    / u128::from(n);
            if u128::from(samples) != boundary {
                bail!("global sample/frame clock mismatch");
            }
            index += 1;
        }
        proofs.push(proof);
    }
    if index != manifest.segments.len() {
        bail!("incomplete programme part coverage");
    }
    Ok(proofs)
}

/// Finalize only explicitly declared compact/PCM/proof/control inputs in a new
/// job. Original lossless parts remain immutable lifecycle dependencies.
pub fn finalize(set_path: &Path, root: &Path, output: &Path) -> Result<Receipt> {
    if output.exists() {
        bail!("programme output must be a new directory");
    }
    let scope = checkpoint::snapshot_job_scope(root)?;
    declared_control(set_path, root, &scope)?;
    if fs::metadata(set_path)?.len() > 4 * 1024 * 1024 {
        bail!("programme part set too large");
    }
    let set: PartSet = serde_json::from_slice(&fs::read(set_path)?)?;
    let bytes = fs::read(scene_delivery::checked_file(root, &set.manifest)?)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let proofs = checked_proofs(&set, root, &bytes)?;
    let parent = output.parent().context("output parent missing")?;
    let stage = tempfile::Builder::new()
        .prefix(".programme-")
        .tempdir_in(parent)?;
    let scratch = tempfile::tempdir_in(stage.path())?;
    let joined_pcm = scratch.path().join("continuous.pcm");
    let mut writer = fs::File::create(&joined_pcm)?;
    let mut concat = String::new();
    let mut records = Vec::new();
    let mut retained = Vec::new();
    let mut hashes = Vec::new();
    let mut counts = Vec::new();
    let mut boundary_findings = Vec::new();
    let (n, d) = manifest
        .output_frame_rate
        .as_ref()
        .context("clock missing")?
        .clock()?;
    let mut last_sample: Option<[i32; 2]> = None;
    let mut last_picture: Option<(PathBuf, u64, String)> = None;
    for proof in &proofs {
        for part in &proof.parts {
            let pcm = scene_delivery::checked_file(root, &part.pcm)?;
            let mut file = fs::File::open(&pcm)?;
            let mut first = [0; 6];
            file.read_exact(&mut first)?;
            let sample = |bytes: &[u8]| -> [i32; 2] {
                let get = |i| i32::from_le_bytes([0, bytes[i], bytes[i + 1], bytes[i + 2]]) >> 8;
                [get(0), get(3)]
            };
            if let Some(previous) = last_sample {
                let next = sample(&first);
                let jump = (0..2)
                    .map(|i| f64::from((previous[i] - next[i]).abs()) / 8388608.0)
                    .fold(0.0, f64::max);
                boundary_findings.push(episode_delivery::BoundaryFinding {
                    left_scene: records
                        .last()
                        .map(|r: &SegmentReceipt| r.id.clone())
                        .unwrap_or_default(),
                    right_scene: part.native.id.clone(),
                    code: "audio-step".into(),
                    measured: jump,
                    threshold: 0.1,
                    disposition: if jump > 0.1 {
                        "needs-review"
                    } else {
                        "measured"
                    }
                    .into(),
                });
            }
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::End(-6))?;
            file.read_exact(&mut first)?;
            last_sample = Some(sample(&first));
            file.rewind()?;
            // Copy while hashing; this proves the exact PCM immediately before
            // the one continuous lossy encoder, including all silent hold bytes.
            let mut hash = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hash.update(&buffer[..read]);
                writer.write_all(&buffer[..read])?;
            }
            if hash
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                != part.pcm.sha256
            {
                bail!("PCM changed during continuous assembly");
            }
            let video = scene_delivery::checked_file(root, &part.video)?;
            if let Some((previous, frame, id)) = &last_picture {
                let black = episode_delivery::black_edge(
                    previous,
                    *frame,
                    proof.facts.width as u32,
                    proof.facts.height as u32,
                )? || episode_delivery::black_edge(
                    &video,
                    0,
                    proof.facts.width as u32,
                    proof.facts.height as u32,
                )?;
                if black {
                    boundary_findings.push(episode_delivery::BoundaryFinding {
                        left_scene: id.clone(),
                        right_scene: part.native.id.clone(),
                        code: "black-at-cut".into(),
                        measured: 1.0,
                        threshold: 0.98,
                        disposition: "needs-review".into(),
                    });
                }
            }
            last_picture = Some((
                video.clone(),
                part.native.frames - 1,
                part.native.id.clone(),
            ));
            let name = video
                .to_str()
                .context("non UTF8 video path")?
                .replace('\\', "/");
            if name.contains(['\n', '\r']) {
                bail!("unsupported concat path character");
            }
            let name = name.replace('\'', "'\\''");
            concat.push_str(&format!(
                "file '{name}'\nduration {:.12}\n",
                part.native.frames as f64 * d as f64 / n as f64
            ));
            counts.push(part.native.frames);
            hashes.push(part.compact_yuv420_sha256.clone());
            records.push(serde_json::from_value(serde_json::to_value(&part.native)?)?);
            retained.push(serde_json::from_value(serde_json::to_value(
                &part.lossless,
            )?)?);
        }
    }
    drop(writer);
    let total_frames = counts.iter().sum();
    let total_samples = records.iter().map(|r| r.samples).sum();
    if fs::metadata(&joined_pcm)?.len() != total_samples * 6 {
        bail!("continuous PCM count mismatch");
    }
    let list = scratch.path().join("video.concat");
    fs::write(&list, concat)?;
    let target = stage.path().join("movie.mp4");
    let profile = compact_profile(&manifest)?;
    let mut cmd = command("ffmpeg");
    cmd.args([
        "-v", "error", "-nostdin", "-f", "concat", "-safe", "0", "-i",
    ])
    .arg(&list)
    .args(["-f", "s24le", "-ar"])
    .arg(manifest.output_sample_rate.to_string())
    .args(["-ac", "2", "-i"])
    .arg(&joined_pcm)
    .args([
        "-map",
        "0:v:0",
        "-map",
        "1:a:0",
        "-c:v",
        "copy",
        "-c:a",
        "libmp3lame",
        "-b:a",
    ])
    .arg(format!("{}k", profile.audio_bitrate_kbps))
    .args(["-video_track_timescale"])
    .arg(n.to_string())
    .args(["-movflags", "+faststart"])
    .arg(&target);
    run(cmd)?;
    let actual = crate::decoded_video_hash::hash(&target, "yuv420p", None, Some(&counts))?;
    let first = &proofs[0].facts;
    if actual.values != hashes || actual.bytes != total_frames * first.width * first.height * 3 / 2
    {
        bail!("final ordered compact decode differs");
    }
    video_clock(&target, total_frames, n, d)?;
    verify_mp3(&target, manifest.output_sample_rate, total_samples)?;
    if video_signature(&target)? != proofs[0].parts[0].video_signature {
        bail!("final video stream signature changed");
    }
    for source in &scope {
        scene_delivery::checked_file(root, source)?;
    }
    let receipt = Receipt {
        schema: RECEIPT.into(), episode_id: manifest.episode_id, language: manifest.language, manifest_sha256: sha(&bytes), output_sample_rate: manifest.output_sample_rate,
        total_frames, total_samples, timestamps_verified: true, compact_ordered_content_verified: true, preencode_pcm_identity_verified: true,
        lossless_master: LosslessMaster::OrderedImmutableParts { parts: retained, custody: "prior native bounded producer proofs; unseen lossless bytes were not freshly rehashed by finalizer; retain and pin every cache URI".into() },
        delivery: reference(stage.path(), "movie.mp4")?, segments: records, consumed_proofs: set.proofs,
        finalizer_job_sha256: file_sha(&root.join("job.json"))?, boundary_findings,
        limitations: vec!["Independent per-part GOP/rate control resets; selected CRF/profile parity, no continuous-encoder pixel or packet equivalence".into(), "One continuous MP3 encode; final decoded MP3 is lossy and does not claim lossless PCM equality; codec tail is bounded to two MP3 frames".into(), "Producer-applied technical checks; human listening, picture/cut/caption review, rights and release approval remain open".into()], publication: "not-authorized".into(),
    };
    fs::write(
        stage.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    drop(scratch);
    fs::rename(stage.keep(), output)?;
    Ok(receipt)
}

fn verify_mp3(path: &Path, rate: u32, samples: u64) -> Result<()> {
    let out = command("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_streams",
            "-show_frames",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !out.status.success() {
        bail!("MP3 decode probe failed");
    }
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let stream = &value["streams"][0];
    if stream["codec_name"] != "mp3"
        || stream["sample_rate"].as_str() != Some(&rate.to_string())
        || stream["channels"] != 2
    {
        bail!("final audio is not selected stereo MP3");
    }
    let mut cursor = 0u64;
    for frame in value["frames"]
        .as_array()
        .context("MP3 decoded frames missing")?
    {
        let time: f64 = frame["best_effort_timestamp_time"]
            .as_str()
            .context("MP3 timestamp missing")?
            .parse()?;
        if !time.is_finite() || (time - cursor as f64 / f64::from(rate)).abs() > 0.002 {
            bail!("MP3 priming/clock discontinuity");
        }
        cursor = cursor
            .checked_add(
                frame["nb_samples"]
                    .as_u64()
                    .context("MP3 samples missing")?,
            )
            .context("MP3 count overflow")?;
    }
    if cursor < samples || cursor - samples > 2304 {
        bail!("MP3 decoded endpoint outside codec tail bound");
    }
    let duration: f64 = value["format"]["duration"]
        .as_str()
        .context("MP4 duration missing")?
        .parse()?;
    let expected = samples as f64 / f64::from(rate);
    if !duration.is_finite()
        || duration < expected - 0.002
        || duration > expected + 2304.0 / f64::from(rate) + 0.002
    {
        bail!("MP4 duration outside complete programme/MP3-tail clock");
    }
    Ok(())
}
