# Dialogue-anchored score mixing and stem delivery v0.3.12

REEL v0.3.12 extends the existing manifest audio timeline. It executes declared
mix intent and records evidence; it does not choose cues, compose, identify
instruments, decide creative balance, approve a mix, or claim a Golden.

## Stable graph and compatibility

The enforced graph is:

```text
source trim/loop
  -> base event gain + event-local automation
  -> event fades
  -> role buses
  -> optional declared dynamic processing
  -> ordered target-specific ducking
  -> bus sum
  -> declared mastering/limiter
  -> exact runtime conform
```

All new fields are optional. A manifest using only the original four roles and
`narration_ducking` takes the legacy compiler path, retaining its original
filter graph and audio-policy hash shape. `narration_ducking` normalizes only
for new stem compilation as detector `[narration]` targeting every non-narration
role, without a new reduction floor. A manifest cannot declare both legacy and
generalized ducking.

## Dialogue and phrase-level automation

`dialogue` is a distinct event role; it does not alter `narration`. D routing is
`narration + dialogue`. A speech detector is authored by listing either or both
roles in a ducking policy.

```yaml
audio_events:
  - id: score
    role: music
    source: audio/score.wav
    start_seconds: 0
    duration_seconds: 12
    gain_db: -9
    gain_automation:
      - { time_seconds: 0.0, gain_db: -2, curve: smooth }
      - { beat_marker_id: question, gain_db: -8, curve: linear }
      - { beat_marker_id: reaction, gain_db: 1, curve: smooth }
  - id: line
    role: dialogue
    source: audio/line.wav
    start_seconds: 3
    duration_seconds: 4
```

Every automation point has exactly one anchor. `time_seconds` is event-local;
a beat marker resolves from timeline time to event-local time. Resolved times
must be finite, unique, strictly ascending, and inside the event. `gain_db` is
additive to the event base gain. The point's curve governs the segment from
that point to the next: `hold`, `linear`, or deterministic smoothstep. Before
the first and after the last point, the nearest point value holds.

## Target-specific ducking

```yaml
audio_ducking:
  - id: speech-over-score
    detector_roles: [narration, dialogue]
    target_roles: [music]
    threshold: 0.03
    ratio: 3.0
    max_reduction_db: 6.0
    attack_ms: 25
    release_ms: 350
```

Policies execute in manifest order. IDs and role entries must be unique;
detectors and targets must be disjoint and present; target roles cannot appear
in more than one policy. A target set must remain within one D, M, or E stem so
stem lineage stays unambiguous. The dry-floor blend enforces
`max_reduction_db`. Ambience and effects remain untouched unless named.

Optional `dynamic_eq` fields are `frequency_hz`, `q`, `max_cut_db`, `attack_ms`,
and `release_ms`. REEL validates and serializes that engine-neutral plan against
the declared detector and target roles. v0.3.12 does not claim portable render
support: a non-dry render fails explicitly, while `animatic-audio-render
--dry-run` exposes the plan with `dynamic_eq_render_supported: false`.

REEL v0.3.13 implements the previously gated render path without changing this
schema. See `speech-keyed-dynamic-eq-v0.3.13.md` for processing order, bounds,
and portability evidence.

## Stem package and review variants

```powershell
reel animatic-audio-render manifest.yaml `
  --asset-root . `
  --output review.m4a `
  --stems-dir review/stems `
  --sample-rate-hz 48000 `
  --channels 2 `
  --format json
```

The stem directory must not exist. Defaults are 48 kHz, stereo, 24-bit PCM WAV.
It contains:

- `dialogue.pre-master.wav` (D = narration + dialogue)
- `music.pre-master.wav` (M, after declared music ducking)
- `effects.pre-master.wav` (E = ambience + effects)
- `mix.pre-master.wav` (the declared D+M+E sum)
- `mix.mastered.wav` (declared mastering/limiter applied)
- `review.no-score.wav`, `review.mono.wav`, and
  `review.small-speaker.wav` (180 Hz–5.5 kHz mono proxy)
- `receipt.json`

D/M/E, pre-master, and mastered full mix share the exact start, sample count,
sample rate, and channel layout. Mono review outputs intentionally use one
channel while retaining exact duration/sample count. The path-free receipt
binds manifest/policy/source/tool/output hashes, resolved automation, normalized
ducking, sample geometry, and a PCM24 proof that `D + M + E` equals pre-master
within three least-significant bits (the bound for independent quantization).
`animatic-audio-check` rechecks hashes, geometry, receipt lineage, and the
recombination calculation. Outputs and receipts are never overwritten.

## Optional dialogue-anchored evidence

Targets are owner-authored manifest policy, never REEL creative defaults:

```yaml
audio_review_policy:
  id: episode-delivery
  dialogue_loudness_target_lufs: -20
  dialogue_loudness_tolerance_lu: 2
  minimum_speech_to_background_margin_db: 6
  speech_activity_threshold_dbfs: -45
  maximum_mono_loss_db: 3
```

When present, the stem receipt records EBU-R128 loudness measured on the D stem,
minimum D-to-(M+E) RMS margin in deterministic 100 ms speech-active windows,
mono downmix loss, mastered clipping, and a non-silent small-speaker proxy. A
failed target remains explicit evidence and does not imply human rejection or
approval.
