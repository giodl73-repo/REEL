//! Bounded mono PCM take clocks; no decoding, resampling or file access.
use anyhow::{Result, ensure};
pub fn mono_pcm_wave_clock(raw: &[u8]) -> Result<(u32, u64)> {
    ensure!(
        (44..=128 * 1024 * 1024).contains(&raw.len())
            && &raw[..4] == b"RIFF"
            && &raw[8..12] == b"WAVE",
        "PCM WAVE required"
    );
    ensure!(
        u32::from_le_bytes(raw[4..8].try_into()?) as usize + 8 == raw.len(),
        "RIFF length differs"
    );
    let mut cursor = 12usize;
    let mut format = None;
    let mut data = None;
    let mut chunks = 0;
    while cursor < raw.len() {
        chunks += 1;
        ensure!(chunks <= 128 && cursor + 8 <= raw.len(), "WAVE chunk bound");
        let size = u32::from_le_bytes(raw[cursor + 4..cursor + 8].try_into()?) as usize;
        let start = cursor + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("WAVE chunk overflow"))?;
        ensure!(end <= raw.len(), "WAVE chunk truncated");
        match &raw[cursor..cursor + 4] {
            b"fmt " => {
                ensure!(
                    format.is_none() && size >= 16,
                    "WAVE format duplicate or truncated"
                );
                let u16at = |n| u16::from_le_bytes([raw[start + n], raw[start + n + 1]]);
                let rate = u32::from_le_bytes(raw[start + 4..start + 8].try_into()?);
                let byte_rate = u32::from_le_bytes(raw[start + 8..start + 12].try_into()?);
                let block = u16at(12);
                let bits = u16at(14);
                let pcm = u16at(0) == 1
                    || (u16at(0) == 0xfffe
                        && size >= 40
                        && u16at(16) >= 22
                        && u16at(18) == bits
                        && matches!(
                            u32::from_le_bytes(raw[start + 20..start + 24].try_into()?),
                            0 | 4
                        )
                        && raw[start + 24..start + 40]
                            == [
                                1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
                            ]);
                ensure!(
                    pcm && u16at(2) == 1
                        && rate > 0
                        && matches!((bits, block), (16, 2) | (24, 3))
                        && rate.checked_mul(u32::from(block)) == Some(byte_rate),
                    "mono PCM16/24 format required"
                );
                format = Some((rate, block));
            }
            b"data" => {
                ensure!(data.is_none() && size > 0, "WAVE data duplicate or empty");
                data = Some(size);
            }
            _ => (),
        }
        cursor = end
            .checked_add(size % 2)
            .ok_or_else(|| anyhow::anyhow!("WAVE padding overflow"))?;
        ensure!(cursor <= raw.len(), "WAVE padding missing");
    }
    let (rate, block) = format.ok_or_else(|| anyhow::anyhow!("WAVE format absent"))?;
    let data = data.ok_or_else(|| anyhow::anyhow!("WAVE samples absent"))?;
    ensure!(data % usize::from(block) == 0, "partial PCM frame");
    Ok((rate, (data / usize::from(block)) as u64))
}
