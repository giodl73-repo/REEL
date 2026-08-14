# Scoped voice direction and prosody evidence in CLI v0.2.16

REEL v0.2.16 separates emotional intent from measurable cadence. It extends
`reel.voice-performance.v0.1` additively, so existing v0.2.15 sidecars remain
valid, with optional per-span fields:

```yaml
emotion_scope: onset
baseline_register: speaker-reference
pitch_contour: falling
terminal_boundary: decisive-fall
relative_pitch_target_semitones: { start: 0, middle: -1, end: -3 }
join_after: protected-pause
```

The controlled vocabularies are:

- emotion scope: `whole-span`, `onset`, `body`, `terminal`;
- baseline register: `speaker-reference`, `lower`, `level`, `higher`;
- pitch contour: `level`, `rising`, `falling`, `rise-fall`, `fall-rise`;
- terminal boundary: `open`, `suspended`, `decisive-fall`, `question-rise`;
- join: `seamless`, `natural`, `protected-pause`.

Relative targets are bounded to ±24 semitones and interpreted relative to the
span's requested starting point. Contradictory terminal/contour requests,
seamless joins with pauses, and protected-pause joins without pauses fail
validation.

## Honest engine compilation

`chatterbox`, `indextts25`, and `generic` are accepted engine identifiers.
REEL does not claim that any current adapter natively guarantees pitch contour,
terminal boundary, register, emotional scope, relative targets or joins. Those
dimensions remain `advisory-only` in the compiled plan. Chatterbox retains its
bounded native intensity conditioning; IndexTTS 2.5 is named explicitly but is
not credited with controls REEL has not implemented and verified.

## Measured result evidence

An analyzer outside REEL measures each rendered span and supplies a strict,
path-free input:

```yaml
schema: reel.voice-prosody-measurements.v0.1
plan_sha256: <sha256-of-plan.json>
rendered_audio_sha256: <sha256-of-rendered-audio>
analyzer: pyin
analyzer_version: librosa-x.y
spans:
  - span_id: decisive-action
    start_seconds: 4.0
    end_seconds: 5.0
    median_f0_hz: 188
    first_f0_hz: 200
    middle_f0_hz: 188
    final_f0_hz: 168
    voiced_frame_coverage: 0.8
    duration_seconds: 1.0
```

REEL binds those measurements to the exact plan and rendered-audio hash:

```powershell
reel voice-prosody-evidence performance-plan measurements.yaml rendered.wav `
  --output-dir prosody-evidence --output json

reel voice-prosody-evidence-check prosody-evidence performance-plan `
  measurements.yaml rendered.wav --output json
```

`evidence.json` contains no text or local paths. It reports robust three-part F0
summaries, relative semitone movement, detected contour, requested-versus-
observed matches, voiced-frame coverage, and a per-span `passed`, `failed`, or
`advisory-only` status. A requested decisive fall that actually rises remains a
visible failure. Measurements must follow exact plan order, cover every span,
bind the rendered-audio hash, and provide ordered nonoverlapping time bounds.
Coverage below 25 percent or duration below 200 ms cannot pass as reliable
direction evidence. Rechecking detects changed plans, plan receipts,
measurements, audio or computed findings.

This evidence verifies acoustic direction only. It never proves emotion, age,
gender, cultural authenticity, speaker identity or human approval, and every
packet keeps `human_listening_required: true`.
