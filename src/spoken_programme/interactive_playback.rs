//! Checked PCM delivery for the local interaction session.
use super::{Session, checker::CheckedPackage, producer::read_source};
use crate::scene_delivery::InputFiles;
use anyhow::{Result, anyhow, ensure};

const FRAME_BYTES: usize = 6;
const MAX_WAVE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Default)]
pub struct Playback {
    visit: u64,
    pcm: Vec<u8>,
}

impl Playback {
    /// Submit up to the requested number of frames to a local sink. The sink
    /// acknowledges the whole submitted chunk before the graph can advance.
    pub fn deliver(
        &mut self,
        session: &mut Session<'_>,
        package: &CheckedPackage,
        candidate: &(impl InputFiles + ?Sized),
        max_frames: usize,
        mut submit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<usize> {
        let result = self.deliver_checked(session, package, candidate, max_frames, &mut submit);
        if result.is_err() {
            session.close();
            *self = Self::default();
        }
        result
    }

    fn deliver_checked(
        &mut self,
        session: &mut Session<'_>,
        package: &CheckedPackage,
        candidate: &(impl InputFiles + ?Sized),
        max_frames: usize,
        submit: &mut impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<usize> {
        let Some(span) = session.selection() else {
            *self = Self::default();
            return Ok(0);
        };
        if session.is_paused() || max_frames == 0 {
            return Ok(0);
        }
        // Reopen at every visit, including a repeat of the same edition.
        if self.visit != session.visit() || self.pcm.is_empty() {
            super::admit_checked_selection(package, span)?;
            let edition = package
                .access()
                .editions
                .iter()
                .find(|e| e.edition_id == span.edition && e.language == span.language)
                .ok_or_else(|| anyhow!("checked edition unavailable"))?;
            ensure!(
                package.access().sample_rate == 48_000 && edition.wave.bytes <= MAX_WAVE_BYTES,
                "interactive PCM profile"
            );
            let wave = read_source(candidate, &edition.wave, &mut || Ok(()))?;
            let pcm = crate::song_edition::wave::pcm24(&wave)?;
            ensure!(
                u64::try_from(pcm.len() / FRAME_BYTES)? == edition.duration_samples,
                "checked PCM duration differs"
            );
            self.pcm = pcm.to_vec();
            self.visit = session.visit();
        }
        let frame = session
            .frame()
            .ok_or_else(|| anyhow!("missing playback cursor"))?;
        let count = usize::try_from(span.end_frame - frame)?.min(max_frames);
        let offset = usize::try_from(frame)? * FRAME_BYTES;
        let end = offset + count * FRAME_BYTES;
        let bytes = self
            .pcm
            .get(offset..end)
            .ok_or_else(|| anyhow!("PCM span unavailable"))?;
        submit(bytes)?;
        ensure!(
            session.observe(self.visit, frame + u64::try_from(count)?),
            "playback observation refused"
        );
        if session.visit() != self.visit {
            *self = Self::default();
        }
        Ok(count)
    }
}
