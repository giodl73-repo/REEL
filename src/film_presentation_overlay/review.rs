//! Private frame-exact cel IDs over a delivered scene. No selection or approval.
use super::{Clocks, Time, clocks_using, file_hash, hash, movie_timescale};
use crate::scene_delivery::{self, FileRef, InputFiles, MediaProgram};
use anyhow::{Context, Result, ensure};
use reel_publication::film_review::SourceFile;
pub use reel_publication::film_review::{Receipt, ReviewMap, Target};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub story_id: String,
    pub cel_id: String,
    pub semantic_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub timeout_seconds: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub probe_bytes: u64,
    pub frames: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub project: String,
    pub source_commit: String,
    pub language: String,
    pub film: FileRef,
    pub delivery_receipt: FileRef,
    pub job: FileRef,
    pub font: FileRef,
    pub font_size: u32,
    /// Keys are exact picture attachment IDs in the actual delivery plan.
    pub identities: BTreeMap<String, Identity>,
    pub limits: Limits,
}
fn source_file(file: &FileRef) -> SourceFile {
    SourceFile {
        path: file.path.clone(),
        sha256: file.sha256.clone(),
        bytes: file.bytes,
    }
}
fn file_reference(file: &SourceFile) -> FileRef {
    FileRef {
        path: file.path.clone(),
        sha256: file.sha256.clone(),
        bytes: file.bytes,
    }
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn control(files: &(impl InputFiles + ?Sized), file: &FileRef, maximum: u64) -> Result<Vec<u8>> {
    ensure!(file.bytes <= maximum, "review control exceeds budget");
    let raw = files.control_bytes(file)?;
    ensure!(
        raw.len() as u64 == file.bytes && hash(&raw) == file.sha256,
        "review control pin mismatch"
    );
    Ok(raw)
}
fn parse(files: &(impl InputFiles + ?Sized), reference: &FileRef) -> Result<(Request, Instant)> {
    let started = Instant::now();
    let q: Request = serde_json::from_slice(&control(files, reference, 1024 * 1024)?)?;
    ensure!(
        q.schema == "reel.film-cel-review.v1"
            && token(&q.project)
            && token(&q.language)
            && [40, 64].contains(&q.source_commit.len())
            && q.source_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            && !q.source_commit.bytes().all(|b| b == b'0')
            && !q.identities.is_empty()
            && q.identities.len() <= 2048,
        "invalid review request identity"
    );
    ensure!(
        (1..=18000).contains(&q.limits.timeout_seconds)
            && (1..=64 * 1024 * 1024 * 1024).contains(&q.limits.input_bytes)
            && (1..=16 * 1024 * 1024 * 1024).contains(&q.limits.output_bytes)
            && (1024..=512 * 1024 * 1024).contains(&q.limits.probe_bytes)
            && (1..=1_000_000).contains(&q.limits.frames)
            && (10..=64).contains(&q.font_size),
        "review limits refuse"
    );
    let mut total = 0u64;
    for file in [&q.film, &q.delivery_receipt, &q.job, &q.font] {
        files.validate_reference_name(file)?;
        ensure!(file.bytes > 0, "empty review input");
        total = total
            .checked_add(file.bytes)
            .context("review input overflow")?;
    }
    ensure!(
        total <= q.limits.input_bytes && q.font.bytes <= 32 * 1024 * 1024,
        "review inputs exceed budget"
    );
    let mut ids = BTreeSet::new();
    for (attachment, id) in &q.identities {
        ensure!(
            token(attachment)
                && token(&id.story_id)
                && token(&id.cel_id)
                && token(&id.semantic_id)
                && ids.insert((&id.story_id, &id.cel_id, &id.semantic_id)),
            "invalid or duplicate review target"
        );
    }
    let deadline = started + Duration::from_secs(q.limits.timeout_seconds);
    Ok((q, deadline))
}
fn run(command: &mut Command, deadline: Instant, maximum: u64) -> Result<Vec<u8>> {
    let (raw, _diagnostics) =
        crate::music_album::process::run(command, deadline, move |mut stdout| {
            let mut kept = Vec::new();
            let mut total = 0u64;
            let mut buffer = [0; 65536];
            loop {
                let n = stdout.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                total = total.saturating_add(n as u64);
                if total <= maximum {
                    kept.extend_from_slice(&buffer[..n]);
                }
            }
            ensure!(total <= maximum, "review tool output exceeds budget");
            Ok(kept)
        })?;
    Ok(raw)
}
fn clocks(
    files: &(impl InputFiles + ?Sized),
    path: &Path,
    q: &Request,
    deadline: Instant,
) -> Result<Clocks> {
    let tool = files.media_program(MediaProgram::Ffprobe)?;
    clocks_using(path, &mut |original| {
        run(
            Command::new(&tool)
                .args(["-protocol_whitelist", "file,pipe"])
                .args(original.get_args()),
            deadline,
            q.limits.probe_bytes,
        )
    })
}
fn map(q: &Request, pin: &str, files: &(impl InputFiles + ?Sized)) -> Result<ReviewMap> {
    let receipt: scene_delivery::Receipt =
        serde_json::from_slice(&control(files, &q.delivery_receipt, 4 * 1024 * 1024)?)?;
    let job: scene_delivery::Job =
        serde_yaml::from_slice(&control(files, &q.job, 4 * 1024 * 1024)?)?;
    let source = receipt
        .outputs
        .get("review.mp4")
        .context("source receipt has no review movie")?;
    // Bind the exact producer output name in its original receipt namespace.
    let expected = q
        .delivery_receipt
        .path
        .parent()
        .context("receipt parent missing")?
        .join(&source.path);
    ensure!(
        receipt.schema == "reel.scene-delivery-receipt.v0.1"
            && job.schema == "reel.scene-delivery.v0.1"
            && receipt.plan.schema == "reel.scene-delivery-plan.v0.1"
            && receipt.content_samples == receipt.plan.duration_samples
            && source.path == Path::new("review.mp4")
            && q.film.path == expected
            && q.film.sha256 == source.sha256
            && q.film.bytes == source.bytes
            && receipt.plan.job_sha256 == q.job.sha256
            && receipt.plan.contract_sha256 == job.contract.sha256
            && receipt.plan.production_sha256 == job.production_manifest_sha256
            && receipt.plan.id == job.id
            && receipt.delivery_frames == receipt.plan.frame_count
            && receipt.delivery_frames <= q.limits.frames
            && receipt.plan.fps_numerator > 0
            && receipt.plan.fps_denominator > 0
            && receipt.plan.pictures.len() == q.identities.len()
            && job.pictures.len() == q.identities.len(),
        "source review receipt/job binding mismatch"
    );
    ensure!(
        job.width >= 160 && job.height >= 64 && job.width <= 7680 && job.height <= 4320,
        "review canvas outside supported bounds"
    );
    let mut targets = Vec::new();
    let mut attachments = BTreeSet::new();
    let mut cursor = 0;
    for span in &receipt.plan.pictures {
        ensure!(
            attachments.insert(span.attachment_id.clone()),
            "duplicate review picture occurrence"
        );
        let id = q
            .identities
            .get(&span.attachment_id)
            .context("review attachment has no stable ID")?;
        let pictures: Vec<_> = job
            .pictures
            .iter()
            .filter(|p| p.attachment_id == span.attachment_id)
            .collect();
        ensure!(
            pictures.len() == 1
                && span.start_frame == cursor
                && span.end_frame > cursor
                && span.end_frame <= receipt.delivery_frames,
            "review picture coverage is ambiguous"
        );
        cursor = span.end_frame;
        files.validate_reference_name(&pictures[0].source)?;
        ensure!(
            pictures[0].source.path.as_os_str().len() <= 4096,
            "review picture reference exceeds budget"
        );
        targets.push(Target {
            attachment_id: span.attachment_id.clone(),
            story_id: id.story_id.clone(),
            cel_id: id.cel_id.clone(),
            semantic_id: id.semantic_id.clone(),
            selected_picture: source_file(&pictures[0].source),
            start_frame: span.start_frame,
            end_frame: span.end_frame,
        });
    }
    ensure!(
        cursor == receipt.delivery_frames && cursor > 0,
        "review picture coverage is incomplete"
    );
    Ok(ReviewMap {
        schema: "reel.film-cel-review-map.v1".into(),
        request_sha256: pin.into(),
        project: q.project.clone(),
        source_commit: q.source_commit.clone(),
        language: q.language.clone(),
        source_film: source_file(&q.film),
        delivery_receipt: source_file(&q.delivery_receipt),
        job: source_file(&q.job),
        frame_count: cursor,
        fps_numerator: receipt.plan.fps_numerator,
        fps_denominator: receipt.plan.fps_denominator,
        width: job.width,
        height: job.height,
        targets,
    })
}
fn label(target: &Target) -> String {
    format!("{} | {}", target.cel_id, target.story_id)
}
fn font(files: &(impl InputFiles + ?Sized), q: &Request, map: &ReviewMap) -> Result<Vec<u8>> {
    let path = files.verified_path(&q.font)?;
    let raw = bounded_read(&path, 32 * 1024 * 1024)?;
    ensure!(
        raw.len() as u64 == q.font.bytes && hash(&raw) == q.font.sha256,
        "review font mismatch"
    );
    let face = ttf_parser::Face::parse(&raw, 0)
        .map_err(|_| anyhow::anyhow!("review font is not a supported TrueType face"))?;
    let width = |text: &str| -> Result<u64> {
        let mut sum = 0u64;
        for c in text.chars() {
            let glyph = face
                .glyph_index(c)
                .context("review font lacks an ID glyph")?;
            sum += u64::from(
                face.glyph_hor_advance(glyph)
                    .context("review font lacks glyph advance")?,
            );
        }
        Ok((sum * u64::from(q.font_size)).div_ceil(u64::from(face.units_per_em())))
    };
    let badge = width("REVIEW")?;
    for target in &map.targets {
        ensure!(
            width(&label(target))? + badge + 32 <= u64::from(map.width),
            "stable cel label does not fit review canvas"
        );
    }
    ensure!(
        q.font_size + 16 < map.height,
        "review ID strip exceeds canvas"
    );
    Ok(raw)
}
fn frame_clock(map: &ReviewMap, clock: &Clocks) -> Result<()> {
    ensure!(
        clock.video_pts.len() as u64 == map.frame_count,
        "source frame count differs from delivery receipt"
    );
    let origin = &clock.video_pts[0];
    let rate = Time::new(
        i128::from(map.fps_numerator),
        i128::from(map.fps_denominator),
    )?;
    for (index, pts) in clock.video_pts.iter().enumerate() {
        let base = origin
            .numerator
            .checked_mul(rate.numerator)
            .context("review frame clock overflow")?;
        let step = (index as i128)
            .checked_mul(rate.denominator)
            .and_then(|n| n.checked_mul(origin.denominator))
            .context("review frame clock overflow")?;
        let expected = Time::new(
            base.checked_add(step)
                .context("review frame clock overflow")?,
            origin
                .denominator
                .checked_mul(rate.numerator)
                .context("review frame clock overflow")?,
        )?;
        ensure!(
            pts == &expected,
            "source frame clock differs from delivery receipt"
        );
    }
    Ok(())
}
fn dimensions(
    files: &(impl InputFiles + ?Sized),
    film: &Path,
    map: &ReviewMap,
    deadline: Instant,
) -> Result<()> {
    let raw = run(
        Command::new(files.media_program(MediaProgram::Ffprobe)?)
            .args(["-protocol_whitelist", "file,pipe"])
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "json",
            ])
            .arg(film),
        deadline,
        65536,
    )?;
    let value: serde_json::Value = serde_json::from_slice(&raw)?;
    ensure!(
        value["streams"][0]["width"] == map.width && value["streams"][0]["height"] == map.height,
        "source geometry differs from delivery job"
    );
    Ok(())
}
fn write_new(path: &Path, raw: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(raw)?;
    file.sync_all()?;
    Ok(())
}
fn bounded_read(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(maximum + 1)
        .read_to_end(&mut raw)?;
    ensure!(
        raw.len() as u64 <= maximum,
        "review evidence exceeds budget"
    );
    Ok(raw)
}
fn filters(q: &Request, map: &ReviewMap) -> String {
    let mut graph = format!(
        "drawbox=x=0:y=0:w=iw:h={}:color=black@0.85:t=fill,drawtext=fontfile=font.ttf:text=REVIEW:expansion=none:fontsize={}:fontcolor=white:x=w-tw-8:y=8",
        q.font_size + 16,
        q.font_size
    );
    for (i, target) in map.targets.iter().enumerate() {
        graph.push_str(&format!(",drawtext=fontfile=font.ttf:textfile=label-{i}.txt:expansion=none:fontsize={}:fontcolor=white:x=8:y=8:enable='gte(n,{})*lt(n,{})'",q.font_size,target.start_frame,target.end_frame));
    }
    graph
}
/// Caller keeps provider claims/handles alive throughout all media reads.
/// This produces ordinary candidate files; native retention remains separate.
pub fn build_with_inputs(
    request: &FileRef,
    files: &(impl InputFiles + ?Sized),
    out: &Path,
) -> Result<Receipt> {
    let (q, deadline) = parse(files, request)?;
    let map = map(&q, &request.sha256, files)?;
    let map_bytes = serde_json::to_vec_pretty(&map)?;
    ensure!(
        map_bytes.len() <= 4 * 1024 * 1024,
        "review target map exceeds budget"
    );
    let font = font(files, &q, &map)?;
    let film = files.verified_path(&q.film)?;
    let source = clocks(files, &film, &q, deadline)?;
    frame_clock(&map, &source)?;
    dimensions(files, &film, &map, deadline)?;
    let timescale = movie_timescale(&source)?.to_string();
    ensure!(
        Instant::now() < deadline,
        "review deadline exhausted before render"
    );
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    write_new(&out.join("font.ttf"), &font)?;
    for (i, target) in map.targets.iter().enumerate() {
        write_new(
            &out.join(format!("label-{i}.txt")),
            label(target).as_bytes(),
        )?;
    }
    write_new(&out.join("filters.txt"), filters(&q, &map).as_bytes())?;
    write_new(&out.join("review-map.json"), &map_bytes)?;
    let movie = out.join("review.mp4");
    run(
        Command::new(files.media_program(MediaProgram::Ffmpeg)?)
            .current_dir(&out)
            .args([
                "-v",
                "error",
                "-nostdin",
                "-n",
                "-protocol_whitelist",
                "file,pipe",
                "-copyts",
                "-i",
            ])
            .arg(&film)
            .args([
                "-map",
                "0:v:0",
                "-map",
                "0:a:0",
                "-/filter:v",
                "filters.txt",
                "-movie_timescale",
                &timescale,
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
                "-fs",
                &q.limits.output_bytes.to_string(),
            ])
            .arg(&movie),
        deadline,
        65536,
    )?;
    ensure!(
        fs::metadata(&movie)?.len() <= q.limits.output_bytes,
        "review output exceeds budget"
    );
    let rendered = clocks(files, &movie, &q, deadline)?;
    ensure!(
        rendered == source,
        "review changes source video timestamps or compressed audio packets"
    );
    dimensions(files, &movie, &map, deadline)?;
    run(
        Command::new(files.media_program(MediaProgram::Ffmpeg)?)
            .args([
                "-v",
                "error",
                "-nostdin",
                "-xerror",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
            ])
            .arg(&movie)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"]),
        deadline,
        65536,
    )?;
    // Recheck through the same guarded provider before recording completion.
    files.verified_path(&q.film)?;
    let mut outputs = BTreeMap::new();
    for name in ["review.mp4", "review-map.json", "font.ttf", "filters.txt"] {
        outputs.insert(
            name.into(),
            SourceFile {
                path: name.into(),
                sha256: file_hash(&out.join(name))?,
                bytes: fs::metadata(out.join(name))?.len(),
            },
        );
    }
    for i in 0..map.targets.len() {
        let name = format!("label-{i}.txt");
        outputs.insert(
            name.clone(),
            SourceFile {
                path: name.clone().into(),
                sha256: file_hash(&out.join(&name))?,
                bytes: fs::metadata(out.join(name))?.len(),
            },
        );
    }
    let receipt = Receipt {
        schema: "reel.film-cel-review-receipt.v1".into(),
        request_sha256: request.sha256.clone(),
        source_clocks_sha256: hash(&serde_json::to_vec(&source)?),
        output_clocks_sha256: hash(&serde_json::to_vec(&rendered)?),
        frame_count: map.frame_count,
        audio_packet_count: source.audio_packets.len() as u64,
        outputs,
        publication: "not-authorized".into(),
        visual_reviewed: false,
        approvals_verified: false,
        native_retention_verified: false,
    };
    write_new(
        &out.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(receipt)
}
pub fn check_with_inputs(
    request: &FileRef,
    files: &(impl InputFiles + ?Sized),
    out: &Path,
) -> Result<Receipt> {
    let (q, deadline) = parse(files, request)?;
    let map = map(&q, &request.sha256, files)?;
    let expected = font(files, &q, &map)?;
    ensure!(
        bounded_read(&out.join("font.ttf"), 32 * 1024 * 1024)? == expected
            && bounded_read(&out.join("filters.txt"), 1024 * 1024)? == filters(&q, &map).as_bytes(),
        "review label/font recipe changed"
    );
    let actual: ReviewMap = serde_json::from_slice(&bounded_read(
        &out.join("review-map.json"),
        4 * 1024 * 1024,
    )?)?;
    ensure!(actual == map, "review target map changed");
    for (i, target) in map.targets.iter().enumerate() {
        ensure!(
            bounded_read(&out.join(format!("label-{i}.txt")), 1024)? == label(target).as_bytes(),
            "review ID text changed"
        );
    }
    let receipt: Receipt =
        serde_json::from_slice(&bounded_read(&out.join("receipt.json"), 4 * 1024 * 1024)?)?;
    let expected_names: BTreeSet<_> = [
        "review.mp4".to_string(),
        "review-map.json".into(),
        "font.ttf".into(),
        "filters.txt".into(),
    ]
    .into_iter()
    .chain((0..map.targets.len()).map(|i| format!("label-{i}.txt")))
    .collect();
    ensure!(
        receipt.outputs.keys().cloned().collect::<BTreeSet<_>>() == expected_names,
        "review output closure changed"
    );
    for (name, file) in &receipt.outputs {
        let maximum = match name.as_str() {
            "review.mp4" => q.limits.output_bytes,
            "font.ttf" => 32 * 1024 * 1024,
            _ => 4 * 1024 * 1024,
        };
        ensure!(
            file.path == Path::new(name) && file.bytes <= maximum,
            "review output identity or budget changed"
        );
        scene_delivery::checked_file(out, &file_reference(file))?;
    }
    let source = clocks(files, &files.verified_path(&q.film)?, &q, deadline)?;
    let rendered = clocks(files, &out.join("review.mp4"), &q, deadline)?;
    frame_clock(&map, &source)?;
    ensure!(
        receipt.schema == "reel.film-cel-review-receipt.v1"
            && receipt.request_sha256 == request.sha256
            && receipt.frame_count == map.frame_count
            && receipt.audio_packet_count == source.audio_packets.len() as u64
            && receipt.source_clocks_sha256 == hash(&serde_json::to_vec(&source)?)
            && receipt.output_clocks_sha256 == hash(&serde_json::to_vec(&rendered)?)
            && source == rendered
            && !receipt.visual_reviewed
            && !receipt.approvals_verified
            && !receipt.native_retention_verified
            && receipt.publication == "not-authorized"
            && fs::metadata(out.join("review.mp4"))?.len() <= q.limits.output_bytes,
        "review receipt/clock closure mismatch"
    );
    dimensions(files, &out.join("review.mp4"), &map, deadline)?;
    run(
        Command::new(files.media_program(MediaProgram::Ffmpeg)?)
            .args([
                "-v",
                "error",
                "-nostdin",
                "-xerror",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
            ])
            .arg(out.join("review.mp4"))
            .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"]),
        deadline,
        65536,
    )?;
    Ok(receipt)
}

/// CLI helper: the request itself is pinned before any source input or effect.
pub fn request_reference(path: &Path, pin: &str) -> Result<FileRef> {
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut raw)?;
    ensure!(
        raw.len() <= 1024 * 1024 && hash(&raw) == pin,
        "review request pin mismatch"
    );
    Ok(FileRef {
        path: path.canonicalize()?,
        sha256: pin.into(),
        bytes: raw.len() as u64,
    })
}
/// Only the explicitly pinned request is outside the producer-relative namespace.
pub struct Local<'a> {
    pub request: &'a FileRef,
    pub root: &'a Path,
}
impl InputFiles for Local<'_> {
    fn verified_path(&self, item: &FileRef) -> Result<PathBuf> {
        if item == self.request {
            ensure!(
                file_hash(&item.path)? == item.sha256
                    && fs::metadata(&item.path)?.len() == item.bytes,
                "review request changed"
            );
            Ok(item.path.clone())
        } else {
            self.root.verified_path(item)
        }
    }
    fn validate_reference_name(&self, item: &FileRef) -> Result<()> {
        if item == self.request {
            Ok(())
        } else {
            self.root.validate_reference_name(item)
        }
    }
}
