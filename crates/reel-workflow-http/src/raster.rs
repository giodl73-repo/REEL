//! Provider-neutral PNG integrity and bounded pixel inspection. No generation,
//! source/rights authentication, storage custody or creative approval.
use crate::error;
use reel_workflow::{ErrorKind, Result};
use std::io::Cursor;
pub const MAX_IMAGE: u64 = 64 * 1024 * 1024;
#[derive(Clone, Debug, serde::Serialize)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub alpha: bool,
    pub transparent: bool,
}
pub fn inspect_png(raw: &[u8]) -> Result<Info> {
    if raw.len() as u64 > MAX_IMAGE {
        return Err(error(ErrorKind::Budget, "png-bytes"));
    }
    let mut decoder = png::Decoder::new(Cursor::new(raw));
    decoder.set_limits(png::Limits {
        bytes: MAX_IMAGE as usize,
    });
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .map_err(|_| error(ErrorKind::Integrity, "png-header"))?;
    let info = reader.info();
    if info.animation_control.is_some()
        || info.width == 0
        || info.height == 0
        || u64::from(info.width) * u64::from(info.height) > 16_777_216
        || reader.output_buffer_size() > MAX_IMAGE as usize
    {
        return Err(error(ErrorKind::Budget, "png-layout"));
    }
    let mut pixels = vec![0; reader.output_buffer_size()];
    let output = reader
        .next_frame(&mut pixels)
        .map_err(|_| error(ErrorKind::Integrity, "png-decode"))?;
    reader
        .finish()
        .map_err(|_| error(ErrorKind::Integrity, "png-end"))?;
    let channels = match output.color_type {
        png::ColorType::Rgba => Some(4),
        png::ColorType::GrayscaleAlpha => Some(2),
        _ => None,
    };
    let transparent = channels.is_some_and(|n| {
        let sample_bytes = if output.bit_depth == png::BitDepth::Sixteen {
            2
        } else {
            1
        };
        let stride = n * sample_bytes;
        pixels[..output.buffer_size()]
            .chunks_exact(stride)
            .any(|p| p[stride - sample_bytes..].iter().any(|v| *v != 255))
    });
    Ok(Info {
        width: output.width,
        height: output.height,
        alpha: channels.is_some(),
        transparent,
    })
}
