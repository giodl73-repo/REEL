# BERTICA production migration audit

Date: 2026-08-04

REEL ran read-only migrations from the twelve YAML manifests currently present
under `C:/src/bertica/production/reel/` into ignored REEL `target/` derivatives.
No BERTICA source or production file was modified or copied into tracked REEL
content.

## Results

| Family | Manifests | v0.2 migration | v0.2 validation |
|---|---:|---:|---:|
| Don Tancredo wedding | 2 | pass | pass |
| Herrera arriving in Melena | 2 | pass | pass |
| Moro pilot | 2 | pass | pass |
| Nochebuena/Riverita | 2 | pass | pass |
| Papo and Cachita | 2 | pass | pass |
| Voice auditions | 2 | pass | pass |
| **Total** | **12** | **12/12** | **12/12** |

Migration used `--normalize-timing`, which rebuilt shot starts from rounded
millisecond durations and synchronized scenes, platforms, and exports. This
resolved the observed Moro 10 ms accumulated-start disagreement.

The fuller Moro and voice-audition manifests contained shot narration and were
lifted into explicit `legacy-narrator` guide cues. Reduced animatic manifests
without shot narration remained valid timed animatics with no inferred speaker
identity. That is intentional: migration does not manufacture authorship,
consent, source text, or approval.

The ten video manifests migrated to the `animatic` profile. The two voice
auditions migrated to the distinct `voice-audition` profile.

## Renderer proof

REEL also conformed the sanitized two-speaker fixture to 7500 ms and rendered it
through Windows/WSL FFmpeg 6.1.1 using real PNG inputs, silent WAV audio, burned
SRT captions, two pan/hold treatments, a dissolve, and a private-review
disclosure. ffprobe reported exactly 7.500 seconds, and the artifact report
recorded every input hash, output hash, byte count, command argument, and FFmpeg
version.
