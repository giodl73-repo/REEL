//! Checked offline transport core. No device enumeration, playback or capture.
//! Native output and measured device acceptance are separate M12 gates.
use super::{checker, producer};
use crate::scene_delivery::InputFiles;
use anyhow::{Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(feature = "installation-native")]
pub mod native;
mod transport;
pub use transport::{Command, Effects, Notice, Snapshot, State, Trace, Transport, TransportError};

pub const INTERVAL_FRAMES: u64 = 12000;
pub const MAX_CALLBACK_FRAMES: usize = 2048;
pub const MAX_SESSION_MS: u64 = 120000;

/// A narrow API selection, not a new programme or asset manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub edition_id: String,
    pub passes: u8,
}
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub spoken: checker::CheckRequest<'a>,
    pub sources: &'a (dyn InputFiles + Sync),
    pub candidate: &'a (dyn InputFiles + Sync),
    pub access: &'a (dyn InputFiles + Sync),
    pub access_manifest: &'a [u8],
}
/// Sealed original checks plus frozen sample/text bytes. This grants no output
/// capability, device qualification, listening decision or release authority.
pub struct Prepared<'a> {
    selection: Selection,
    inputs: Inputs<'a>,
    samples: Arc<[f32]>,
    transcript: Arc<str>,
    seal: [u8; 32],
}
/// An original-check result for one restart generation. Private fields and no
/// deserializer; obtain it off the controller/callback, then consume it once.
/// A fixed-size value needs no allocation or destruction on the callback.
pub struct RestartPermit {
    session: u64,
    generation: u64,
    seal: [u8; 32],
}
/// Opaque in-process ticket for one transport restart. Not a serialized
/// capability or a replacement for the native worker's runtime binding.
pub struct RestartTicket {
    session: u64,
    generation: u64,
    seal: [u8; 32],
}
#[derive(Debug, Serialize)]
pub struct Summary {
    pub source_frames: usize,
    pub passes: u8,
    pub transcript_bytes: usize,
    pub original_pcm_and_access_checked: bool,
    pub device_playback_checked: bool,
    pub listening_reviewed: bool,
    pub authority_verified: bool,
    pub release_authorized: bool,
}
impl Prepared<'_> {
    pub fn transcript(&self) -> &str {
        &self.transcript
    }
    pub fn summary(&self) -> Summary {
        Summary {
            source_frames: self.samples.len() / 2,
            passes: self.selection.passes,
            transcript_bytes: self.transcript.len(),
            original_pcm_and_access_checked: true,
            device_playback_checked: false,
            listening_reviewed: false,
            authority_verified: false,
            release_authorized: false,
        }
    }
    /// Recheck originals before opening a transport. This operation opens no
    /// stream; its first state is Ready, not Playing.
    pub fn transport(
        &self,
        now_ms: u64,
        control: producer::ProductionControl<impl FnMut() -> Result<()>>,
    ) -> Result<Transport> {
        let fresh = prepare(&self.selection, self.inputs, control)?;
        ensure!(
            fresh.seal == self.seal,
            "installation original selection changed"
        );
        // The fresh seal proves the frozen PCM/text are still the same. Share
        // that allocation with the transport instead of retaining a second
        // identical playback buffer beside Prepared.
        Ok(Transport::new(
            self.samples.clone(),
            self.transcript.clone(),
            fresh.seal,
            fresh.selection.passes,
            now_ms,
        )?)
    }
    /// Check off the controller/callback. No mutable transport borrow: the
    /// controller can process Stop/Quit while this check is in flight. On error
    /// the host submits RestartRejected for this generation, not a fake permit.
    pub fn restart_permit(
        &self,
        ticket: RestartTicket,
        control: producer::ProductionControl<impl FnMut() -> Result<()>>,
    ) -> Result<RestartPermit> {
        ensure!(
            ticket.seal == self.seal,
            "installation restart selection differs"
        );
        let fresh = prepare(&self.selection, self.inputs, control)?;
        ensure!(
            fresh.seal == self.seal,
            "installation original selection changed"
        );
        Ok(RestartPermit {
            session: ticket.session,
            generation: ticket.generation,
            seal: fresh.seal,
        })
    }
}

fn text(raw: Vec<u8>) -> Result<Arc<str>> {
    ensure!(
        !raw.is_empty() && raw.len() <= 4096,
        "installation transcript byte profile"
    );
    let value = String::from_utf8(raw)?;
    // Conservative one-cell Latin profile, including precomposed Spanish.
    // Refuse unsupported layout; never normalize/rewrite original text.
    ensure!(
        value.chars().all(|c| c == '\n'
            || (!c.is_control()
                && c != '\u{00ad}'
                && (c as u32 <= 0x024f
                    || matches!(
                        c,
                        '\u{2013}' | '\u{2014}' | '\u{2018}' | '\u{2019}' | '\u{201c}' | '\u{201d}'
                    )))),
        "installation transcript terminal character profile"
    );
    ensure!(
        value.lines().count() <= 20 && value.lines().all(|l| l.chars().count() <= 80),
        "installation transcript terminal line profile"
    );
    Ok(value.into())
}

fn pcm24_sample(b: &[u8]) -> f32 {
    let n = i32::from_le_bytes([b[0], b[1], b[2], if b[2] & 128 != 0 { 255 } else { 0 }]);
    n as f32 / 8388608.0
}

pub fn prepare<'a>(
    selection: &Selection,
    inputs: Inputs<'a>,
    mut control: producer::ProductionControl<impl FnMut() -> Result<()>>,
) -> Result<Prepared<'a>> {
    ensure!(
        (1..=3).contains(&selection.passes)
            && !selection.edition_id.trim().is_empty()
            && selection.edition_id.len() <= 256
            && !selection.edition_id.chars().any(char::is_control),
        "installation selection profile"
    );
    ensure!(
        control.timeout <= Duration::from_secs(30) && inputs.access_manifest.len() <= 1024 * 1024,
        "installation metadata profile"
    );
    let deadline = Instant::now() + control.timeout;
    let mut tick = || {
        (control.cancellation)()?;
        ensure!(
            Instant::now() < deadline,
            "installation check deadline elapsed"
        );
        Ok(())
    };
    tick()?;
    let mut checked = checker::check(
        inputs.spoken,
        inputs.sources,
        inputs.candidate,
        producer::ProductionControl {
            timeout: deadline.saturating_duration_since(Instant::now()),
            cancellation: &mut tick,
        },
    )?;
    checker::verify_access(
        &mut checked,
        inputs.access_manifest,
        inputs.access,
        &mut tick,
    )?;
    let editions: Vec<_> = checked
        .access()
        .editions
        .iter()
        .filter(|e| e.edition_id == selection.edition_id)
        .collect();
    ensure!(
        editions.len() == 1,
        "installation selected edition absent or ambiguous"
    );
    let edition = editions[0];
    ensure!(
        (9600..=480000).contains(&edition.duration_samples)
            && checked.access().sample_rate == 48000
            && edition.wave.bytes <= 2880000 + 65536
            && edition.transcript.bytes <= 4096,
        "installation selected source profile"
    );
    let wave = producer::read_source(inputs.candidate, &edition.wave, &mut tick)?;
    let pcm = crate::song_edition::wave::pcm24(&wave)?;
    ensure!(
        pcm.len() as u64 == edition.duration_samples * 6,
        "installation source frame count differs"
    );
    ensure!(
        pcm[..4800 * 6]
            .iter()
            .chain(&pcm[pcm.len() - 4800 * 6..])
            .all(|b| *b == 0),
        "installation source requires quiet head and tail"
    );
    let samples: Arc<[f32]> = pcm.chunks_exact(3).map(pcm24_sample).collect();
    let transcript = text(producer::read_source(
        inputs.access,
        &edition.transcript,
        &mut tick,
    )?)?;
    // Internal original-control equality only; never a portable identity or an
    // admission/authority token. No self-reported receipt replaces checker PCM.
    let seal = Sha256::digest(serde_json::to_vec(&serde_json::json!({
        "plan":crate::production::sha256_bytes(inputs.spoken.plan_bytes),
        "receipt":crate::production::sha256_bytes(inputs.spoken.receipt_bytes),
        "access":crate::production::sha256_bytes(inputs.access_manifest),
        "edition":selection.edition_id,"passes":selection.passes,"wave":edition.wave,"text":edition.transcript
    }))?).into();
    tick()?;
    Ok(Prepared {
        selection: selection.clone(),
        inputs,
        samples,
        transcript,
        seal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm24_sign_and_rails_have_independent_float_bit_oracles() {
        // Raw little-endian bytes and literal IEEE-754 answers. Full-scale
        // rails are conversion cases, not admitted producer/listening fixtures.
        let raw = [
            [0, 0, 128],
            [255, 255, 255],
            [0, 0, 0],
            [1, 0, 0],
            [255, 255, 127],
        ];
        let expected = [0xbf800000, 0xb4000000, 0, 0x34000000, 0x3f7ffffe];
        for (bytes, bits) in raw.iter().zip(expected) {
            assert_eq!(pcm24_sample(bytes).to_bits(), bits);
        }
    }
    #[test]
    fn exact_text_access_without_audio() {
        let spanish = "Después vuelve a empezar. ¡Sigue el ritmo!\n";
        assert_eq!(&*text(spanish.as_bytes().to_vec()).unwrap(), spanish);
        for bad in [
            "",
            "bad\u{1b}[31m",
            "tab\ttext",
            "soft\u{00ad}hyphen",
            "wide界",
            "combined a\u{0301}",
        ] {
            assert!(text(bad.as_bytes().to_vec()).is_err(), "{bad:?}");
        }
        assert!(text(vec![b'a'; 81]).is_err());
        assert!(text("line\n".repeat(21).into_bytes()).is_err());
        assert!(text(vec![b'a'; 4097]).is_err());
        assert!(text(vec![255]).is_err());
    }
}
