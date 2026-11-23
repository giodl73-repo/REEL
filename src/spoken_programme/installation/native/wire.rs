//! Bounded private byte framing, not programme authority or message admission.
//! The session must additionally check its strict message schema, actual OS peer,
//! command applicability and generation. No transport/permit is serialized here.
use anyhow::{Result, ensure};
use std::io::Read;

pub const MAX_FRAME_BYTES: usize = 8192;
pub const HEADER_BYTES: usize = 40;
pub const MAX_PAYLOAD_BYTES: usize = MAX_FRAME_BYTES - HEADER_BYTES;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Command = 1,
    Report = 2,
}
impl Direction {
    pub fn opposite(self) -> Self {
        match self {
            Self::Command => Self::Report,
            Self::Report => Self::Command,
        }
    }
}
/// Deliberately no Debug/Serde: private payloads are not diagnostic output.
pub struct Packet {
    bytes: [u8; MAX_FRAME_BYTES],
    len: usize,
}
impl Packet {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
    pub fn payload(&self) -> &[u8] {
        &self.bytes[HEADER_BYTES..self.len]
    }
    pub fn generation(&self) -> u64 {
        u64::from_le_bytes(self.bytes[32..40].try_into().unwrap())
    }
    pub fn sequence(&self) -> u64 {
        u64::from_le_bytes(self.bytes[24..32].try_into().unwrap())
    }
}
pub struct Encoder {
    nonce: [u8; 16],
    direction: Direction,
    next: u64,
}
impl Encoder {
    pub fn new(nonce: [u8; 16], direction: Direction) -> Result<Self> {
        ensure!(nonce != [0; 16], "installation empty wire context");
        Ok(Self {
            nonce,
            direction,
            next: 1,
        })
    }
    pub fn encode(&mut self, generation: u64, payload: &[u8]) -> Result<Packet> {
        ensure!(
            !payload.is_empty() && payload.len() <= MAX_PAYLOAD_BYTES,
            "installation wire payload bound"
        );
        let next = self
            .next
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("installation wire sequence exhausted"))?;
        let mut packet = Packet {
            bytes: [0; MAX_FRAME_BYTES],
            len: HEADER_BYTES + payload.len(),
        };
        packet.bytes[..4].copy_from_slice(b"RIPC");
        packet.bytes[4] = 1;
        packet.bytes[5] = self.direction as u8;
        packet.bytes[6..8].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        packet.bytes[8..24].copy_from_slice(&self.nonce);
        packet.bytes[24..32].copy_from_slice(&self.next.to_le_bytes());
        packet.bytes[32..40].copy_from_slice(&generation.to_le_bytes());
        packet.bytes[HEADER_BYTES..packet.len].copy_from_slice(payload);
        self.next = next;
        Ok(packet)
    }
}
pub struct Decoder {
    nonce: [u8; 16],
    direction: Direction,
    next: Option<u64>,
}
impl Decoder {
    pub fn new(nonce: [u8; 16], direction: Direction) -> Result<Self> {
        ensure!(nonce != [0; 16], "installation empty wire context");
        Ok(Self {
            nonce,
            direction,
            next: Some(1),
        })
    }
    /// Blocking I/O belongs on the owned reader thread. Check the fixed header
    /// before reading its bounded body. Any error poisons this run's receiver;
    /// a later valid frame cannot reconnect or resynchronize it.
    pub fn read(&mut self, reader: &mut impl Read) -> Result<Packet> {
        let expected = self
            .next
            .take()
            .ok_or_else(|| anyhow::anyhow!("installation wire receiver closed"))?;
        let next = expected
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("installation wire sequence exhausted"))?;
        let mut packet = Packet {
            bytes: [0; MAX_FRAME_BYTES],
            len: HEADER_BYTES,
        };
        reader
            .read_exact(&mut packet.bytes[..HEADER_BYTES])
            .map_err(|_| anyhow::anyhow!("installation wire header unavailable"))?;
        let length = u16::from_le_bytes(packet.bytes[6..8].try_into().unwrap()) as usize;
        ensure!(
            packet.bytes[..4] == *b"RIPC"
                && packet.bytes[4] == 1
                && packet.bytes[5] == self.direction as u8
                && packet.bytes[8..24] == self.nonce
                && packet.sequence() == expected
                && (1..=MAX_PAYLOAD_BYTES).contains(&length),
            "installation wire context/sequence/bound mismatch"
        );
        packet.len += length;
        reader
            .read_exact(&mut packet.bytes[HEADER_BYTES..packet.len])
            .map_err(|_| anyhow::anyhow!("installation wire body unavailable"))?;
        self.next = Some(next);
        Ok(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn exact_private_frame_preserves_spanish_and_independent_sequences() {
        for direction in [Direction::Command, Direction::Report] {
            let mut encoder = Encoder::new([7; 16], direction).unwrap();
            let mut decoder = Decoder::new([7; 16], direction).unwrap();
            for (generation, text) in [(0, "Texto español: acción\n"), (2, "stop"), (1, "status")]
            {
                let packet = encoder.encode(generation, text.as_bytes()).unwrap();
                let result = decoder.read(&mut Cursor::new(packet.as_bytes())).unwrap();
                assert_eq!(result.generation(), generation);
                assert_eq!(result.sequence(), packet.sequence());
                assert_eq!(result.payload(), text.as_bytes());
            }
        }
    }
    #[test]
    fn foreign_duplicate_skipped_oversized_and_unknown_headers_poison_the_run() {
        let packet = Encoder::new([7; 16], Direction::Command)
            .unwrap()
            .encode(1, b"start")
            .unwrap();
        for offset in [0, 4, 5, 6, 7, 8, 24] {
            let mut bad = packet.as_bytes().to_vec();
            bad[offset] ^= 255;
            if offset == 6 {
                bad[6] = 0;
            }
            let mut decoder = Decoder::new([7; 16], Direction::Command).unwrap();
            let mut raw = Cursor::new(bad);
            assert!(decoder.read(&mut raw).is_err());
            assert_eq!(raw.position(), HEADER_BYTES as u64);
            assert!(decoder.read(&mut Cursor::new(packet.as_bytes())).is_err());
        }
        let mut decoder = Decoder::new([7; 16], Direction::Command).unwrap();
        decoder.read(&mut Cursor::new(packet.as_bytes())).unwrap();
        assert!(decoder.read(&mut Cursor::new(packet.as_bytes())).is_err());
    }
    #[test]
    fn truncated_headers_bodies_and_closed_peers_cannot_resume() {
        let packet = Encoder::new([7; 16], Direction::Command)
            .unwrap()
            .encode(1, b"start")
            .unwrap();
        for len in [0, 1, 39, 40, 44] {
            let mut decoder = Decoder::new([7; 16], Direction::Command).unwrap();
            assert!(
                decoder
                    .read(&mut Cursor::new(&packet.as_bytes()[..len]))
                    .is_err()
            );
            assert!(decoder.read(&mut Cursor::new(packet.as_bytes())).is_err());
        }
    }
    #[test]
    fn maximum_frame_fits_and_zero_or_exhausted_context_refuses() {
        assert!(Encoder::new([0; 16], Direction::Command).is_err());
        assert!(Decoder::new([0; 16], Direction::Command).is_err());
        let mut encoder = Encoder::new([3; 16], Direction::Report).unwrap();
        assert!(encoder.encode(0, b"").is_err());
        assert!(encoder.encode(0, &[1; MAX_PAYLOAD_BYTES + 1]).is_err());
        let packet = encoder.encode(0, &[1; MAX_PAYLOAD_BYTES]).unwrap();
        assert_eq!(packet.as_bytes().len(), MAX_FRAME_BYTES);
        assert_eq!(packet.sequence(), 1);
        encoder.next = u64::MAX;
        assert!(encoder.encode(0, b"status").is_err());
    }
}
