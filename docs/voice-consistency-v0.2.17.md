# Cross-scene voice consistency in CLI v0.2.17

REEL v0.2.17 adds a measurable preflight between voice synthesis and full-scene
assembly. It keeps an approved voice identity, performance mode, speaking-rate
band, and minimum cue gap stable across auditions and scenes. A renderer may no
longer treat “measured” or “use the same voice” as an informal note.

The feature is additive. It does not change `reel.manifest.v0.2`, the voice
performance sidecar, or prosody evidence.

## Command

```powershell
reel voice-consistency-check manifest.yaml voice-profile.yaml measurements.yaml `
  --report voice-consistency.json --output json
```

The command exits unsuccessfully when a measured cue is too fast, too slow, or
has a shorter-than-approved following gap. It also rejects stale manifest
bindings, unknown or duplicated cues, incomplete scene measurements, speaker
identity drift, continuity-key drift, and performance-mode drift.

Use `scope: audition` for a short render that samples every speaker in the scene.
Use `scope: scene` for the complete set of narration cues in that scene manifest.
An audition is the economical gate before full synthesis; a complete scene must
still pass independently.

## Approved series profile

```yaml
schema: reel.voice-consistency-profile.v0.1
profile_id: series-voice-v1
approval_reference: docs/decisions/approved-voice.md
speakers:
  - speaker_id: narrator
    continuity_key: narrator-natural-a
    mode: narrator-self
    target_wpm: 120
    minimum_wpm: 108
    maximum_wpm: 132
    minimum_pause_after_ms: 250
    reference_audio_sha256: <64-character-sha256>
    approval_reference: docs/decisions/approved-voice.md
```

`continuity_key` identifies the approved voice treatment, not a local file.
`reference_audio_sha256` binds the profile to the reviewed reference without
exposing its path. `approval_reference` records the human decision; REEL never
infers approval from a passing measurement.

The modes are `narrator-self`, `poet`, and `cast-character`. Each continuity key
may belong to only one speaker profile. This prevents a narrator's self-voice
from silently becoming a cast-character voice or one character voice from being
reused for another.

## Measured candidate

```yaml
schema: reel.voice-consistency-measurements.v0.1
manifest_sha256: <sha256-of-exact-manifest>
scene_id: scene-01
scope: scene
cues:
  - cue_id: cue-01
    speaker_id: narrator
    continuity_key: narrator-natural-a
    mode: narrator-self
    duration_ms: 4700
    head_silence_ms: 100
    tail_silence_ms: 100
    pause_after_ms: 350
```

REEL derives word count from the manifest's exact inline cue text and subtracts
head/tail silence before calculating words per minute. `pause_after_ms` is the
assembled gap after the cue, so deliberate breathing room is checked separately
from silence embedded inside a rendered file.

The retained `reel.voice-consistency-report.v0.1` is strict, path-free JSON. It
contains per-cue and duration-weighted per-speaker rates, deviation from target,
violations, and hashes of all three inputs. It is suitable for binding into a
later artifact receipt.

## Relationship to other REEL gates

- `voice-performance-plan` controls localized dramatic actions.
- `voice-prosody-evidence` checks measured contour intent.
- `voice-consistency-check` protects cross-scene identity, pace, and gaps.
- `audio-check` protects loudness, peaks, silence, and stem margin.

Together these make scene delivery repeatable without pretending that numerical
checks replace human listening. Character emotion can vary within a scene while
the underlying approved identity and pace envelope remain stable.

The sanitized fixtures in `manifests/fixtures/voice-consistency/` include one
passing scene and one scene that fails for fast delivery and short pauses.
