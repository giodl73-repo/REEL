use reel_assembly::audio_clock::mono_pcm_wave_clock;
fn chunk(id: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend((bytes.len() as u32).to_le_bytes());
    v.extend(bytes);
    if bytes.len() % 2 != 0 {
        v.push(0);
    }
    v
}
fn wave(bits: u16, extensible: bool) -> Vec<u8> {
    let block = bits / 8;
    let mut fmt = Vec::new();
    fmt.extend((if extensible { 0xfffeu16 } else { 1u16 }).to_le_bytes());
    fmt.extend(1u16.to_le_bytes());
    fmt.extend(24000u32.to_le_bytes());
    fmt.extend((24000 * u32::from(block)).to_le_bytes());
    fmt.extend(block.to_le_bytes());
    fmt.extend(bits.to_le_bytes());
    if extensible {
        fmt.extend(22u16.to_le_bytes());
        fmt.extend(bits.to_le_bytes());
        fmt.extend(4u32.to_le_bytes());
        fmt.extend([
            1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
        ]);
    }
    let mut body = b"WAVE".to_vec();
    body.extend(chunk(b"JUNK", &[1]));
    body.extend(chunk(b"fmt ", &fmt));
    body.extend(chunk(b"data", &vec![0; usize::from(block) * 3]));
    let mut v = b"RIFF".to_vec();
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(body);
    v
}
#[test]
fn clock_counts_pcm16_and_pcm24_frames_with_padding_and_extensible_headers() {
    for bits in [16, 24] {
        for extensible in [false, true] {
            assert_eq!(
                mono_pcm_wave_clock(&wave(bits, extensible)).unwrap(),
                (24000, 3)
            );
        }
    }
}
#[test]
fn malformed_headers_chunks_and_partial_frames_are_rejected() {
    let good = wave(16, false);
    for index in [0, 4, 8, 16, 30, 32, 34, 38, 42, 44, 46, 50] {
        let mut bad = good.clone();
        bad[index] ^= 0xff;
        assert!(mono_pcm_wave_clock(&bad).is_err(), "byte {index}");
    }
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(mono_pcm_wave_clock(&trailing).is_err());
    for end in 0..good.len() {
        assert!(mono_pcm_wave_clock(&good[..end]).is_err());
    }
    let mut duplicate = good.clone();
    duplicate.extend(chunk(b"data", &[0, 0]));
    let len = duplicate.len() as u32 - 8;
    duplicate[4..8].copy_from_slice(&len.to_le_bytes());
    assert!(mono_pcm_wave_clock(&duplicate).is_err());
}
