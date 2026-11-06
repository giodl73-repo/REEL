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

pub(super) fn decode(raw: &[u8]) -> Result<Wave> {
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
                }
                if !(1..=2).contains(&channels)
                    || !(8000..=96000).contains(&rate)
                    || !matches!((tag, bits), (1, 16 | 24 | 32) | (3, 32))
                    || u16le(chunk, 12) != channels * (bits / 8)
                    || u32le(chunk, 8) != rate * u32::from(channels) * u32::from(bits / 8)
                {
                    bail!("unsupported WAVE clock or PCM format");
                }
                format = Some((tag, channels, rate, bits));
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
    let (tag, channels, rate, bits) =
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
        float_data: if tag == 3 { data_range } else { None },
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
