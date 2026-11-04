# Calibrated sample playback on a native score clock

`reel music-sample-clock-render REQUEST --output-dir NEW_ABSOLUTE_DIRECTORY`
renders a private, unreviewed PCM24 instrumental and receipt. It uses existing
calibrated samples and native REEL score MIDI; it starts no sound renderer,
server, separator, download or cache operation.

The request schema is `reel.music-sample-clock-render.v1`. Required fields:

- `bank_audio`, `calibration`, `native_score_midi`: exact `{path,sha256,bytes}`
  references to existing absolute files. Each file is captured and rehashed.
- `sample_rate`, `channels`, `frames`: the unchanged source clock and complete
  output length. The initial sample standard is IEEE float32 WAVE, mono/stereo,
  8–192 kHz. Unsupported encodings and clock/channel mismatches refuse.
- `release_frames`: explicit quadratic key-release tail, at most one second.
  Tails truncate only at the declared output endpoint. Samples must cover each
  complete held note plus its admitted tail; no looping, stretching or pitching.
- `gain`: explicit finite gain, 0–16. Velocity scales a fixed-64 calibration by
  `(velocity/64)^1.5`. `peak_ceiling` is strictly between zero and one; a final
  attenuation-only scale enforces it, with no automatic boost.
- Optional `vocal`: `{source: {path,sha256,bytes}, gain}`. The existing float32
  recording must match the declared rate, channels and frame count exactly.
  Gain is explicit; no implicit trim, padding, channel conversion, resampling,
  pitch processing or vocal regeneration occurs. One common attenuation keeps
  each stem and their mix below the ceiling, with PCM24 rounding headroom.

The calibration schema `reel.music-sample-bank.v1` contains a `bank` array:
`pitch`, `audio_attack_frame`, `held_sample_frames`, and optional provenance
`renderer_start_seconds`. The same existing fields in retained
`reel.piano-sample-clock-evidence.v1` are accepted. Old verification/review flags
do not become new assertions. The request pins both the exact calibration
metadata and its original audio. Each pitch must be unique and audible, its
window must fit the bank, and every requested pitch must be present.

Native MIDI means the existing REEL format-1 conductor/part contract, not an
arbitrary application round trip. Its exact integer tempo/tick clock rounds to
the nearest frame, with halves upward. Part-track tempo changes, unmarked parts,
duplicate part markers, incomplete notes and notes outside the output refuse.
Overlapping unisons on different voice channels remain independent notes.
Structural controls are checked before opening larger audio. Input bytes and
prior renders are never changed.

All input, note, track, mix-work and output-sample allocations are bounded.
The bank is at most 128 MiB, each metadata/MIDI input 1 MiB, the score 16,384
notes across at most 64 parts, and output 32 Mi interleaved samples. This admits
full four-minute stereo material at 44.1 kHz; it does not relax a managed job's
smaller declared output or memory allowance.
Unknown fields, duplicate JSON keys, stale input pins, missing/short/inaudible
calibrations and non-finite audio refuse before output creation. An existing
output refuses. I/O failure may leave an incomplete new owned directory; a final
receipt and the normal managed retention workflow are required before adoption.

The output contains `instrumental.wav` and a private `receipt.json` with exact
input pins, request/output hashes, native part/voice/note clocks, velocity and
release policy, gain and attenuation. Stdout contains counts and explicit
non-approval/non-publication flags. With `vocal`, it also writes `vocal-stem.wav`
and `with-vocals.wav`. The master is the exact integer sum of the separately
quantized PCM24 stems, preserving stem-sum equality without clipping. Silent
quantized results and invalid vocal controls refuse before output creation.
Retain the receipt and output through the
project's normal admitted asset workflow; this command supplies no custody,
selection, public-release or eviction authority.

This implements sample playback and declared stem mixing, not harmony estimation,
sound-bank capture, notation approval or complete managed arrangement production.
The calibrated bank's pitch/timbre attribution and rights require their own
source evidence. Musical listening and selection remain human decisions.

## Read-only checking

`reel music-sample-clock-check REQUEST DIRECTORY` captures the same pinned
request and primary identities, checks the receipt against the native score
schedule, and verifies each output hash and PCM24 clock. With a vocal source it
also checks every quantized vocal sample against the original source and declared
gain/attenuation, and checks exact integer stem-sum equality. It does not rerender,
write files, infer clean separation, or grant musical approval.

The check reports its explicit coverage. It does not claim an independent
primary-reference waveform reconstruction of every instrumental note, current
Tig selection eligibility, or complete semantic coverage. The existing b4
instrumental-only packet format remains readable through its original output
pin. Source-aware managed adoption remains a separate operation.
