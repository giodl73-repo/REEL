//! Separate bounded completion stages; only the final stage issues a receipt.
use super::*;

const DIRECTORY: &str = "verified-lossless";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Verified {
    schema: String,
    state: String,
    validator_sha256: String,
    ffmpeg_sha256: String,
    ffprobe_sha256: String,
    job_sha256: String,
    preparation_sha256: String,
    lossless_receipt: Receipt,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Encoded {
    schema: String,
    verification_sha256: String,
    movie: scene_delivery::FileRef,
    full_decode_verified: bool,
}

fn closed_workspace(prepared: &Path, assets: &Path) -> Result<()> {
    if prepared.canonicalize()?.parent() != Some(assets.canonicalize()?.as_path()) {
        bail!("staged completion requires preparation directly inside its closed workspace");
    }
    Ok(())
}

fn bounded_bytes(path: &Path) -> Result<Vec<u8>> {
    if fs::metadata(path)?.len() > 4 * 1024 * 1024 {
        bail!("completion checkpoint exceeds its bound");
    }
    Ok(fs::read(path)?)
}

pub(super) fn verify_preparation(prepared: &Path, assets: &Path) -> Result<()> {
    closed_workspace(prepared, assets)?;
    // Uses precisely the existing ordered picture/PCM, clock and boundary gates.
    // The partial result remains wrapped, with compact delivery explicitly pending.
    finish_mode(prepared, assets, &prepared.join(DIRECTORY), true, true)?;
    Ok(())
}

pub(super) fn save_verified(
    prepared: &Path,
    assets: &Path,
    preparation: &[u8],
    receipt: &Receipt,
    output: &Path,
) -> Result<()> {
    closed_workspace(prepared, assets)?;
    let verified = Verified {
        schema: "reel.episode-lossless-verification.v1".into(),
        state: "compact-delivery-pending".into(),
        validator_sha256: file_sha(&std::env::current_exe()?)?,
        ffmpeg_sha256: tool_hash("ffmpeg")?,
        ffprobe_sha256: tool_hash("ffprobe")?,
        job_sha256: file_sha(&assets.join("job.json"))?,
        preparation_sha256: sha(preparation),
        lossless_receipt: serde_json::from_value(serde_json::to_value(receipt)?)?,
    };
    fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(&verified)?,
    )?;
    Ok(())
}

struct Loaded {
    manifest: Manifest,
    prepared: Prepared,
    receipt: Receipt,
    master: PathBuf,
    directory: PathBuf,
    verification_sha256: String,
    preparation_bytes: Vec<u8>,
}

fn load_verified(prepared: &Path, assets: &Path) -> Result<Loaded> {
    closed_workspace(prepared, assets)?;
    let directory = prepared.join(DIRECTORY);
    if directory.canonicalize()?.parent() != Some(prepared.canonicalize()?.as_path()) {
        bail!("completion checkpoint escapes its preparation");
    }
    let verification_bytes = bounded_bytes(&directory.join("verification.json"))?;
    let verified: Verified = serde_json::from_slice(&verification_bytes)?;
    let preparation_bytes = bounded_bytes(&prepared.join("prepared.json"))?;
    let checkpoint: Prepared = serde_json::from_slice(&preparation_bytes)?;
    let executable = file_sha(&std::env::current_exe()?)?;
    if verified.schema != "reel.episode-lossless-verification.v1"
        || verified.state != "compact-delivery-pending"
        || verified.validator_sha256 != executable
        || checkpoint.validator_sha256 != executable
        || verified.ffmpeg_sha256 != tool_hash("ffmpeg")?
        || verified.ffprobe_sha256 != tool_hash("ffprobe")?
        || checkpoint.ffmpeg_sha256 != verified.ffmpeg_sha256
        || checkpoint.ffprobe_sha256 != verified.ffprobe_sha256
        || verified.job_sha256 != file_sha(&assets.join("job.json"))?
        || verified.preparation_sha256 != sha(&preparation_bytes)
        || bounded_bytes(&directory.join("preparation.json"))? != preparation_bytes
        || checkpoint.schema != "reel.episode-conform-preparation.v1"
        || checkpoint.full_episode_verified
        || checkpoint.manifest_sha256 != sha(checkpoint.manifest_json.as_bytes())
        || checkpoint.source_scope.is_empty()
        || checkpoint.source_scope.len() > 65536
    {
        bail!("lossless verification identity, source preparation or tool changed");
    }
    for source in &checkpoint.source_scope {
        scene_delivery::checked_file(assets, source)?;
    }
    if !checkpoint
        .source_scope
        .iter()
        .any(|source| source.path == Path::new("job.json") && source.sha256 == verified.job_sha256)
    {
        bail!("lossless verification lacks its exact native job scope");
    }
    let manifest: Manifest = serde_json::from_str(&checkpoint.manifest_json)?;
    compact_profile(&manifest)?;
    let receipt = verified.lossless_receipt;
    if receipt.schema != "reel.episode-conform-receipt.v1"
        || receipt.episode_id != manifest.episode_id
        || receipt.language != manifest.language
        || receipt.manifest_sha256 != checkpoint.manifest_sha256
        || receipt.preparation_sha256.as_deref() != Some(verified.preparation_sha256.as_str())
        || receipt.episode_presentation_fingerprint_sha256
            != checkpoint.presentation_fingerprint_sha256
        || receipt.output_sample_rate != manifest.output_sample_rate
        || receipt.total_frames != checkpoint.frame_cursor
        || receipt.total_samples != checkpoint.sample_cursor
        || receipt.master_sha256 != checkpoint.master.file.sha256
        || receipt.master_bytes != checkpoint.master.file.bytes
        || receipt.encoded_delivery.is_some()
        || !receipt.decoded_master_matches_ordered_segments
        || !receipt.timestamps_verified
        || receipt.publication != "not-authorized"
        || receipt.creative_review_state != "open; no principal approval inferred"
        || serde_json::to_value(&receipt.segments)? != serde_json::to_value(&checkpoint.records)?
    {
        bail!("lossless verification differs from its selected preparation");
    }
    let master = resolve(&checkpoint.master, prepared, assets)?;
    let facts = probe(&master)?;
    if facts.width != checkpoint.first.width
        || facts.height != checkpoint.first.height
        || facts.fps != checkpoint.first.fps
        || facts.sample_rate != manifest.output_sample_rate
        || facts.sample_aspect_ratio != checkpoint.first.sample_aspect_ratio
        || facts.color_properties != checkpoint.first.color_properties
    {
        bail!("verified master media properties changed");
    }
    Ok(Loaded {
        manifest,
        prepared: checkpoint,
        receipt,
        master,
        directory,
        verification_sha256: sha(&verification_bytes),
        preparation_bytes,
    })
}

pub(super) fn encode_verified(prepared: &Path, assets: &Path) -> Result<()> {
    let verified = load_verified(prepared, assets)?;
    let movie = verified.directory.join("episode.mp4");
    if movie.exists() || verified.directory.join("encoding.json").exists() {
        bail!("verified encoding already exists; classify the existing attempt");
    }
    encode_compact(
        &verified.manifest,
        &verified.prepared.first,
        &verified.master,
        &movie,
    )?;
    let checkpoint = Encoded {
        schema: "reel.episode-encoding-checkpoint.v1".into(),
        verification_sha256: verified.verification_sha256,
        movie: scene_delivery::FileRef {
            path: "episode.mp4".into(),
            sha256: file_sha(&movie)?,
            bytes: fs::metadata(&movie)?.len(),
        },
        full_decode_verified: false,
    };
    let pending = verified.directory.join("encoding.pending.json");
    fs::write(&pending, serde_json::to_vec_pretty(&checkpoint)?)?;
    fs::rename(pending, verified.directory.join("encoding.json"))?;
    Ok(())
}

pub(super) fn finish_encoded(prepared: &Path, assets: &Path, output: &Path) -> Result<Receipt> {
    if output.exists() {
        bail!("episode output must be a new directory");
    }
    let mut verified = load_verified(prepared, assets)?;
    let encoding_bytes = bounded_bytes(&verified.directory.join("encoding.json"))?;
    let encoding: Encoded = serde_json::from_slice(&encoding_bytes)?;
    if encoding.schema != "reel.episode-encoding-checkpoint.v1"
        || encoding.verification_sha256 != verified.verification_sha256
        || encoding.full_decode_verified
        || encoding.movie.path != Path::new("episode.mp4")
    {
        bail!("encoding checkpoint differs from its lossless verification");
    }
    let movie = scene_delivery::checked_file(&verified.directory, &encoding.movie)?;
    verified.receipt.encoded_delivery = Some(verify_compact(
        &verified.manifest,
        &verified.prepared.first,
        verified.prepared.frame_cursor,
        verified.prepared.sample_cursor,
        &movie,
    )?);
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if parent.canonicalize()? != assets.canonicalize()? {
        bail!("completion output must be directly inside its closed workspace");
    }
    let stage = tempfile::Builder::new()
        .prefix(".episode-encoded-finish-")
        .tempdir_in(parent)?;
    let target = stage.path().join("episode.mp4");
    fs::copy(&movie, &target)?;
    if file_sha(&target)? != encoding.movie.sha256
        || fs::metadata(&target)?.len() != encoding.movie.bytes
    {
        bail!("completed encoding copy differs from its verified source");
    }
    if compact_profile(&verified.manifest)?.retain_lossless_master {
        let target = stage.path().join("master.mkv");
        fs::copy(&verified.master, &target)?;
        if file_sha(&target)? != verified.receipt.master_sha256
            || fs::metadata(&target)?.len() != verified.receipt.master_bytes
        {
            bail!("completed lossless copy differs from its verified source");
        }
    }
    fs::write(
        stage.path().join("receipt.json"),
        serde_json::to_vec_pretty(&verified.receipt)?,
    )?;
    fs::write(
        stage.path().join("preparation.json"),
        verified.preparation_bytes,
    )?;
    fs::copy(
        verified.directory.join("verification.json"),
        stage.path().join("verified-lossless.json"),
    )?;
    fs::write(stage.path().join("encoding.json"), encoding_bytes)?;
    fs::rename(stage.path(), output)?;
    Ok(verified.receipt)
}
