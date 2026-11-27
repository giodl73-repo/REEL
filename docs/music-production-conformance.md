# Music production conformance

The `reel-music` crate can compare a production policy with validated recording
observations. REEL resolves the analysis back to the exact source contract
before checking it. Label and artist are supplied by the caller, along with a
digest of its intent materials. The caller resolves label defaults, artist
requirements, track briefs and explicit exceptions into the policy.

```powershell
cargo run --locked -p reel-music --bin reel-music-conformance -- policy.json analysis.json
```

A policy uses schema `reel.music-conformance-policy.v1`. Give it a track ID,
label, artist, source contract SHA-256, intent SHA-256, and the standards that
apply to the track.

- **Duration** compares decoded sample count and sample rate with a target and
  tolerance in milliseconds.
- **Tempo** compares eligible timestamped observations with a target and
  tolerance in milli-BPM. A policy can allow half- or double-pulse
  equivalence. Mixed or out-of-range eligible candidates are reported as
  conflicting evidence. A rubato scene can leave the tempo standard unset.
- **Instrumentation** compares exact normalized labels and explicit aliases.
  A track-wide artist commitment and a track-specific request both ask for
  positive source evidence. Ensemble palette entries are context only. An
  input with no eligible matching observation stays unverified.

Tempo and instrumentation policies select analyzer fingerprints and minimum
confidence. `conformance::analyzer_fingerprint` derives the pin from the
adapter, version, model revision and parameter hash. Build that list from
calibration evidence. For saxophone, include examples with and without sax
and test confusion with clarinet and trumpet. Isolated and masked passages
exercise different parts of the intended detection task. An eligible positive
observation establishes reported presence over its sample range. A separate
musical review is needed to show a recognizable theme or a featured solo.

The report preserves the policy and analysis hashes and the accepted
observation IDs and source sample ranges. It produces production findings.
Artistic ratings for groove, phrasing, development and payoff belong to the
listening rubric. The CLI runs offline and consumes existing REEL evidence;
it does not execute a transcription or instrument-recognition model.

## Optional local tools

Use a `reel.music-extensions.v1` registry to configure these tools. The
`installations` map keys are `muscriptor`, `demucs`, `pop2piano`, `ffmpeg`,
and `musescore`. Each installation provides a version and a `files` map.
Every file entry provides an absolute path, SHA-256 and byte count. Additional
runtime files such as interpreter, processor and soundfont assets can also be
pinned. The crate reports which entries are unconfigured, invalid or verified.

| Extension | Required file keys | Evidence |
| --- | --- | --- |
| MuScriptor | executable, weights, config | Raw time-stamped candidate notes and instrument labels |
| Demucs | executable, adapter, weights, config | Estimated audio stems |
| Pop2Piano | executable, adapter, weights, config | Piano reduction candidates |
| FFmpeg | executable | Audio decoding and preparation |
| MuseScore | executable | Notation and deliberate instrument control renders |

MuScriptor uses its installed CLI and local weights with the sibling model
config. Demucs supports the CAIMITOS lossless soundfile CLI adapter and the
local htdemucs repository format. Pop2Piano supports the CAIMITOS single
private request adapter. Its model config and processor files need to be
available in the pinned local snapshot. FFmpeg and MuseScore are exposed
through the Rust registry for use by evidence producers.

```powershell
reel-music-conformance extensions registry.json
reel-music-conformance capture-muscriptor registry.json excerpt.wav SHA256 BYTES events.json
reel-music-conformance run-demucs registry.json excerpt.wav estimated-stems
reel-music-conformance run-pop2piano registry.json SOURCE_ROOT request.json reduction
```

The capture command stages the exact local WAV and writes raw, unprompted
MuScriptor events with an input and installation identity. The model runner
requests CPU and offline mode. Demucs uses a private staged local checkpoint.
The legacy PyTorch loading exception is limited to the exact retained
htdemucs checkpoint. The tools must be installed by the operator. Registry
verification checks declared files and does not establish detector accuracy.

The songbook precedent is to compare the original mix, estimated stems, and
note transcription against the same source clock. Demucs has a residual
`other` stream that can still mix several instruments. Pop2Piano proposes
an arrangement. A MuScriptor target-conditioned pass answers a different
question from an unprompted instrument test. Keep those evidence types
distinct when deciding whether a production standard is supported.

### Compare note candidates across perspectives

```powershell
reel-music-conformance compare-captures mix.json other.json trumpet soprano_and_alto_sax 120
```

Supply captures that share a source clock, such as a full excerpt and an
aligned estimated stem. The report retains both capture hashes, both audio
hashes and one-to-one matches at the same MIDI pitch within the requested
number of milliseconds. It compares note starts even if the analyzer assigns
different labels. Keep the agreed note candidates available for phrase review.
The common-clock binding and instrument identity need independent evidence.
