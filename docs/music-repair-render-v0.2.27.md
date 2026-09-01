# Deterministic music-repair rendering in CLI v0.2.27

REEL v0.2.27 implements Slice B of the music-reconstruction proposal. It turns
a validated cut-only repair into a canonical sample-indexed EDL, renders a new
raw-PCM candidate through the existing FFmpeg adapter, and writes strict local
evidence before declaring the candidate verified.

```powershell
reel music-repair-compile repair.yaml --output-path repair-edl.json --output json
reel music-repair-render repair-edl.json repair.yaml `
  --output-pcm candidate.raw --evidence-path evidence.json --output json
reel music-repair-evidence-check evidence.json repair-edl.json repair.yaml `
  candidate.raw --output json
```

## Executable boundary

`reel.music-repair-edl.v0.1` executes only ordered `cut` operations. `keep` and
`lock` remain declarative constraints; every other typed v0.2.26 operation is
rejected as planned but not executable. Each cut must exactly equal one changed
envelope, must leave signal on both sides, and must preserve the complete source
mapping outside the cuts.

The EDL binds the raw repair manifest, its canonical contract, the source
manifest and contract, decoded PCM identity, format, complete timebase, resolved
keep segments, output ranges, cut joins, and evidence policy. Local absolute
paths make it deliberately non-shareable. EDL publication is atomic and never
overwrites an existing file.

## Rendering

The root CLI owns process execution. It invokes FFmpeg with an explicit raw-PCM
demuxer and encoder, uses `atrim` with integer `start_sample`/`end_sample`
boundaries, resets timestamps, and concatenates the resolved keep segments.
No resampling, gain, fade, denoise, model, network request, or provider SDK is
part of this slice.

Rendering targets a temporary file in the destination directory and publishes
the completed candidate by rename. Existing candidate and evidence paths are
rejected before FFmpeg runs. If evidence thresholds fail, the completed
candidate and failed evidence remain available for private diagnosis and the
command exits unsuccessfully.

## Evidence contract

`reel.music-repair-evidence.v0.1` records integer-scaled metrics and hashes, not
audio or source paths. It proves:

- exact byte identity for every mapped segment outside the declared cuts;
- exact output byte length and samples per channel;
- normalized sample delta at each new boundary;
- left/right window RMS difference;
- waveform-window cosine correlation (not a claim of phase reconstruction);
- normalized short-window spectral distance and DC-offset difference; and
- exact identity and minimum length of the right-side retained tail.

The strict v0.1 policy uses a 256-sample bounded window, a 0.15 normalized
boundary-delta ceiling, 2 dB RMS-difference ceiling, 0.8 minimum window
correlation, 0.2 maximum normalized spectral distance, and at least 16 exact
right-tail samples. These are deterministic engineering gates, not perceptual
listening approval or a universal definition of an inaudible seam.

The evidence JSON is path-free but remains marked `shareable: false` because
signal hashes and repair lineage can still be sensitive. Human review and any
release decision remain separate governed artifacts.

## Verification

Unit tests generate unsigned periodic 8-bit PCM and prove canonical compilation,
exact outside-region identity, passing seam/tail evidence, deliberate outside-
region mutation failure, and rejection of unsupported operations. An explicit
external-adapter integration test compiles and renders the fixture through
FFmpeg, asserts byte-for-byte expected output, and rechecks the saved evidence.

No BERTICA audio, lyrics, titles, paths, identities, or creative judgments are
present in the implementation or tests. Stem separation, transcription,
language adaptation, score reconstruction, re-orchestration, perceptual review,
and comparison packaging remain later governed slices.
