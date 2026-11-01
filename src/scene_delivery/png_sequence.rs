//! Bounded primary PNG sequence admission and ephemeral physical-path controls.
use super::*;
use image::ImageDecoder;
use std::io::Write;

pub(crate) const CONTROL_BYTES: u64 = 1024 * 1024;

pub(crate) fn validate_counts(job: &Job) -> Result<()> {
    let mut total = 0usize;
    for layer in &job.external_layers {
        if layer.render_mode != ExternalLayerRenderMode::TimedPngSequenceOverlay
            && !layer.frames.is_empty()
        {
            bail!("only PNG sequence overlays may declare frames");
        }
        total = total
            .checked_add(layer.frames.len())
            .context("PNG frame count overflow")?;
        if total > 4096 {
            bail!("PNG sequence frame references exceed budget");
        }
    }
    Ok(())
}

pub(super) fn validate(
    layer: &ExternalLayer,
    width: u32,
    height: u32,
    frames: u64,
    inputs: &(impl InputFiles + ?Sized),
) -> Result<()> {
    if layer.frames.is_empty()
        || layer.frames.len() > 4096
        || layer.frames.len() as u64 != frames
        || layer.frames.first() != Some(&layer.evidence)
        || layer.font.is_some()
        || layer.render_source.is_some()
        || layer.derivation_receipt.is_some()
    {
        bail!("PNG sequence must bind exactly one primary image per selected frame");
    }
    index_bytes(layer)?;
    let mut color = None;
    let mut control_bytes = "ffconcat version 1.0\n".len();
    for frame in &layer.frames {
        inputs.validate_reference_name(frame)?;
        let path = inputs.verified_path(frame)?;
        control_bytes = control_bytes
            .checked_add(control_line(&path)?.len())
            .context("PNG control length overflow")?;
        if control_bytes as u64 > CONTROL_BYTES {
            bail!("PNG sequence control exceeds budget");
        }
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(8 * 1024 * 1024);
        limits.max_image_width = Some(width);
        limits.max_image_height = Some(height);
        let png = image::codecs::png::PngDecoder::with_limits(
            std::io::BufReader::new(fs::File::open(path)?),
            limits,
        )?;
        if png.is_apng()? || !png.color_type().has_alpha() || png.dimensions() != (width, height) {
            bail!("PNG sequence requires nonanimated alpha images at exact canvas geometry");
        }
        if color
            .replace(png.color_type())
            .is_some_and(|previous| previous != png.color_type())
        {
            bail!("PNG sequence pixel formats must remain constant");
        }
    }
    Ok(())
}

pub(crate) fn index_bytes(layer: &ExternalLayer) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(&serde_json::json!({
        "schema":"reel.primary-png-sequence.v1", "frames":layer.frames,
    }))?;
    if raw.len() as u64 > CONTROL_BYTES {
        bail!("PNG sequence index exceeds budget");
    }
    Ok(raw)
}

/// Private physical locators are never returned as provenance or receipt data.
pub(crate) fn prepare(
    inputs: &(impl InputFiles + ?Sized),
    root: &Path,
    frames: &[FileRef],
) -> Result<tempfile::NamedTempFile> {
    let mut text = String::from("ffconcat version 1.0\n");
    for frame in frames {
        let path = inputs.verified_path(frame)?;
        text.push_str(&control_line(&path)?);
        if text.len() as u64 > CONTROL_BYTES {
            bail!("PNG sequence control exceeds budget");
        }
    }
    let mut file = tempfile::Builder::new()
        .prefix(".png-sequence-")
        .tempfile_in(root)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    Ok(file)
}

fn control_line(path: &Path) -> Result<String> {
    let value = path.to_str().context("PNG sequence path is not UTF-8")?;
    if !path.is_absolute() || value.chars().any(char::is_control) {
        bail!("PNG sequence physical path is unsupported");
    }
    Ok(format!("file '{}'\n", value.replace('\'', "'\\''")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concat_controls_refuse_relative_paths_and_control_character_injection() {
        assert!(control_line(Path::new("relative.png")).is_err());
        for character in ['\n', '\r', '\t', '\0'] {
            let path = std::env::temp_dir().join(format!("frame{character}file 'foreign.png"));
            assert!(control_line(&path).is_err());
        }
    }
}
