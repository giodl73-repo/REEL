# Executable voice-performance direction in CLI v0.2.15

REEL v0.2.15 adds a strict, provider-neutral performance sidecar without
changing `reel.manifest.v0.2`. It solves a production failure that speaker
identity, exact text, punctuation and general prose direction could all be
correct while the synthesis engine still received no executable instruction
for an interruption, warning, suspense hinge or comic release.

```powershell
reel voice-performance-plan manifest.yaml performance.yaml `
  --engine chatterbox --engine-version 0.1.7 `
  --reference-audio private-reference.wav --seed 1947 `
  --output-dir performance-plan --output json

reel voice-performance-plan-check performance-plan manifest.yaml performance.yaml `
  --reference-audio private-reference.wav --output json
```

## Sidecar contract

`reel.voice-performance.v0.1` binds to the exact manifest SHA-256 and to inline
narration-cue text. Every directed cue is completely covered by nonoverlapping
Unicode character spans. Each span hashes its exact substring and declares a
controlled action, normalized intensity/energy, pace, pitch shape, onset,
optional breathiness, protected pauses and stress tokens.

The action vocabulary covers neutral narration, intimate recollection, comic
aside, breathless plea, exasperated demand, explosive interruption, wounded
dignity, precise counterattack, dangerous threat, fear-driven warning, suspense
build, suspended decision, physical effort, astonished release and dry comic
button. A language/register such as `es-CU` / `intimate-family-storytelling`
provides directing context but never claims one stereotyped performance
represents a culture or every speaker.

Validation rejects unknown cues, stale manifest or substring hashes, duplicate
IDs, gaps, overlaps, invalid bounds, out-of-range intensity values, stress words
absent from their exact span, and contradictory boundary pauses. Output is
published atomically only after validation succeeds.

## Executed versus advisory direction

The plan explicitly distinguishes what an engine can execute from what remains
human direction. The Chatterbox compiler maps intensity into bounded
`exaggeration` and `cfg_weight`, preserves exact phrase boundaries and pauses,
and supplies deterministic post-render tempo targets. Because the selected
engine does not natively guarantee a requested dramatic action, exact pitch
contours, hard/soft onset, individual-word stress, energy, breathiness or
culturally authentic cadence, those dimensions are marked `advisory-only`
instead of being reported as executed.

The generic engine still emits deterministic segmentation, pauses and tempo
targets, but truthfully reports all engine-native expressive dimensions as
advisory. Every plan requires human listening; measured pitch or energy can
later provide evidence but can never prove emotion or cultural authenticity.

## Privacy-safe receipt

The path-free receipt contains hashes of the manifest, performance sidecar,
optional reference audio and compiled plan; engine/version and seed; cue/span
counts; executed and advisory dimensions; and the required human-listening
flag. It contains no cue text, stress words, speaker identity or local paths.
`voice-performance-plan-check` detects changes to any bound input.

This command compiles direction; it does not claim that synthesis occurred.
Consequently this planning receipt has no rendered-audio hash or duration. A
later render-result receipt must bind those measurements before a produced
chunk can replace reviewed audio.

The sanitized fixture in `manifests/fixtures/voice-performance/` demonstrates a
0.95 explosive interruption, fear-driven warning, suspense pause, decisive
action and dry comic button. No consumer manuscript, voice or private asset is
stored in REEL.
