//! Bounded ordinary RIFF WAVE decoding; no external decoder or resampling.
use anyhow::{Result, bail};

pub(super) struct Wave {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
    pub float_data: Option<std::ops::Range<usize>>,
}
fn u16le(raw: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(raw[at..at + 2].try_into().unwrap())
}
fn u32le(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}

struct Layout {
    tag: u16,
    bits: u16,
    rate: u32,
    channels: u16,
    channel_mask: Option<u32>,
    data_range: std::ops::Range<usize>,
}

fn layout(raw: &[u8]) -> Result<Layout> {
    if raw.len() < 12
        || &raw[..4] != b"RIFF"
        || &raw[8..12] != b"WAVE"
        || u32le(raw, 4) as usize + 8 != raw.len()
    {
        bail!("invalid bounded RIFF WAVE");
    }
    let mut offset = 12usize;
    let mut format = None;
    let mut data = None;
    let mut data_range = None;
    let mut count = 0;
    while offset < raw.len() {
        count += 1;
        if count > 512 || offset + 8 > raw.len() {
            bail!("WAVE chunk budget")
        }
        let size = u32le(raw, offset + 4) as usize;
        let end = offset
            .checked_add(8)
            .and_then(|n| n.checked_add(size))
            .ok_or_else(|| anyhow::anyhow!("WAVE size overflow"))?;
        let chunk = raw
            .get(offset + 8..end)
            .ok_or_else(|| anyhow::anyhow!("truncated WAVE chunk"))?;
        match &raw[offset..offset + 4] {
            b"fmt " => {
                if format.is_some() || chunk.len() < 16 {
                    bail!("duplicate or truncated WAVE format")
                }
                let mut tag = u16le(chunk, 0);
                let mut channel_mask = None;
                let channels = u16le(chunk, 2);
                let rate = u32le(chunk, 4);
                let bits = u16le(chunk, 14);
                if tag == 0xfffe {
                    const GUID_TAIL: [u8; 14] =
                        [0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113];
                    if chunk.len() < 40
                        || u16le(chunk, 16) < 22
                        || chunk.len() < 18 + usize::from(u16le(chunk, 16))
                        || u16le(chunk, 18) != bits
                        || chunk[26..40] != GUID_TAIL
                    {
                        bail!("unsupported extensible WAVE format");
                    }
                    tag = u16le(chunk, 24);
                    channel_mask = Some(u32le(chunk, 20));
                }
                if !(1..=2).contains(&channels)
                    || !(8000..=96000).contains(&rate)
                    || !matches!((tag, bits), (1, 16 | 24 | 32) | (3, 32))
                    || u16le(chunk, 12) != channels * (bits / 8)
                    || u32le(chunk, 8) != rate * u32::from(channels) * u32::from(bits / 8)
                {
                    bail!("unsupported WAVE clock or PCM format");
                }
                format = Some((tag, channels, rate, bits, channel_mask));
            }
            b"data" => {
                if data.is_some() {
                    bail!("duplicate WAVE data")
                };
                data = Some(chunk);
                data_range = Some(offset + 8..end);
            }
            _ => {}
        }
        offset = end
            .checked_add(size % 2)
            .ok_or_else(|| anyhow::anyhow!("WAVE padding overflow"))?;
    }
    let (tag, channels, rate, bits, channel_mask) =
        format.ok_or_else(|| anyhow::anyhow!("WAVE format missing"))?;
    let data = data.ok_or_else(|| anyhow::anyhow!("WAVE data missing"))?;
    let width = usize::from(bits / 8);
    if offset != raw.len()
        || data.is_empty()
        || data.len() % (width * usize::from(channels)) != 0
        || data.len() / (width * usize::from(channels)) > rate as usize * 600
    {
        bail!("WAVE frame bounds")
    }
    Ok(Layout {
        tag,
        bits,
        rate,
        channels,
        channel_mask,
        data_range: data_range.ok_or_else(|| anyhow::anyhow!("WAVE data range missing"))?,
    })
}

/// Borrow exact normalized source bytes using the same bounded RIFF validation.
pub(crate) fn pcm24(raw: &[u8]) -> Result<&[u8]> {
    let value = layout(raw)?;
    if (value.tag, value.bits, value.rate, value.channels) != (1, 24, 48_000, 2)
        || value.channel_mask.is_some_and(|mask| mask != 3)
    {
        bail!("source requires exact 48kHz stereo PCM24 WAVE");
    }
    Ok(&raw[value.data_range])
}

pub(super) fn decode(raw: &[u8]) -> Result<Wave> {
    let Layout {
        tag,
        bits,
        rate,
        channels,
        data_range,
        ..
    } = layout(raw)?;
    let data = &raw[data_range.clone()];
    let width = usize::from(bits / 8);
    let mut samples = Vec::with_capacity(data.len() / width);
    for bytes in data.chunks_exact(width) {
        let sample = match (tag, bits) {
            (3, 32) => f32::from_le_bytes(bytes.try_into().unwrap()),
            (1, 16) => f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.0,
            (1, 24) => {
                let n = i32::from_le_bytes([
                    bytes[0],
                    bytes[1],
                    bytes[2],
                    if bytes[2] & 128 != 0 { 255 } else { 0 },
                ]);
                n as f32 / 8388608.0
            }
            (1, 32) => i32::from_le_bytes(bytes.try_into().unwrap()) as f32 / 2147483648.0,
            _ => unreachable!(),
        };
        if !sample.is_finite() {
            bail!("nonfinite source WAVE sample")
        }
        samples.push(sample);
    }
    Ok(Wave {
        rate,
        channels,
        samples,
        float_data: if tag == 3 { Some(data_range) } else { None },
    })
}

pub(super) fn encode(samples: &[f32], rate: u32, channels: u16) -> Result<Vec<u8>> {
    if !(8000..=96000).contains(&rate)
        || !(1..=2).contains(&channels)
        || samples.is_empty()
        || samples.len() % usize::from(channels) != 0
        || samples.len() / usize::from(channels) > rate as usize * 600
        || samples.iter().any(|s| !s.is_finite())
    {
        bail!("invalid float WAVE output")
    }
    let size = u32::try_from(
        samples
            .len()
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("WAVE size"))?,
    )?;
    let mut bytes = Vec::with_capacity(size as usize + 56);
    bytes.extend(b"RIFF");
    bytes.extend((size + 48).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(3u16.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * u32::from(channels) * 4).to_le_bytes());
    bytes.extend((channels * 4).to_le_bytes());
    bytes.extend(32u16.to_le_bytes());
    bytes.extend(b"fact");
    bytes.extend(4u32.to_le_bytes());
    bytes.extend((samples.len() as u32 / u32::from(channels)).to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    for sample in samples {
        bytes.extend(sample.to_le_bytes())
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_pcm24_extensible_requires_left_right_channel_mask() {
        let mut raw = b"RIFF".to_vec();
        raw.extend(66u32.to_le_bytes());
        raw.extend(b"WAVEfmt ");
        raw.extend(40u32.to_le_bytes());
        raw.extend(0xfffeu16.to_le_bytes());
        raw.extend(2u16.to_le_bytes());
        raw.extend(48000u32.to_le_bytes());
        raw.extend(288000u32.to_le_bytes());
        raw.extend(6u16.to_le_bytes());
        raw.extend(24u16.to_le_bytes());
        raw.extend(22u16.to_le_bytes());
        raw.extend(24u16.to_le_bytes());
        raw.extend(3u32.to_le_bytes());
        raw.extend([1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
        raw.extend(b"data");
        raw.extend(6u32.to_le_bytes());
        raw.extend([7, 0, 0, 249, 255, 255]);
        assert_eq!(pcm24(&raw).unwrap(), [7, 0, 0, 249, 255, 255]);
        for mask in [0u32, 1, 12] {
            raw[40..44].copy_from_slice(&mask.to_le_bytes());
            assert!(pcm24(&raw).is_err());
            // The existing broad song decoder retains its prior acceptance;
            // only the new exact stereo borrow path rejects unspecified/remapped channels.
            assert!(decode(&raw).is_ok());
        }
    }
    #[test]
    fn float_bits_round_trip_and_container_damage_refuses() {
        let samples = [-0.0, 0.0, -1.0, 0.5, 1.0, -0.125];
        let bytes = encode(&samples, 8000, 2).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!((decoded.rate, decoded.channels), (8000, 2));
        for (a, b) in samples.iter().zip(decoded.samples) {
            assert_eq!(a.to_bits(), b.to_bits())
        }
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        let mut bad = bytes.clone();
        bad[4] = 0;
        assert!(decode(&bad).is_err());
        let mut bad = bytes;
        bad[56..60].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&bad).is_err());
    }
    #[test]
    fn integer_pcm_is_normalized_at_original_sample_clock() {
        for (bits, data) in [
            (16, vec![0, 128, 0, 64]),
            (24, vec![0, 0, 128, 0, 0, 64]),
            (32, vec![0, 0, 0, 128, 0, 0, 0, 64]),
        ] {
            let width = bits / 8;
            let mut raw = b"RIFF".to_vec();
            raw.extend((36u32 + data.len() as u32).to_le_bytes());
            raw.extend(b"WAVEfmt ");
            raw.extend(16u32.to_le_bytes());
            raw.extend(1u16.to_le_bytes());
            raw.extend(1u16.to_le_bytes());
            raw.extend(8000u32.to_le_bytes());
            raw.extend((8000u32 * width).to_le_bytes());
            raw.extend((width as u16).to_le_bytes());
            raw.extend((bits as u16).to_le_bytes());
            raw.extend(b"data");
            raw.extend((data.len() as u32).to_le_bytes());
            raw.extend(data);
            assert_eq!(decode(&raw).unwrap().samples, vec![-1.0, 0.5]);
        }
    }
}
