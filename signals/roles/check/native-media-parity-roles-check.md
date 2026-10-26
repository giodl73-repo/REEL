# Native media comparison producer review

Scope: generic decoded comparison, bounded reference reader and optional Tig
comparison command. Editor, sound-designer and rights/provenance checklists are
applied by the producer; this is not independent review or human approval.

- Editor: compare every ordered frame, both YUV planes and displayed RGB, plus
  native dimensions, rational rate and per-frame clock. Container identity does
  not substitute for decoded content. Unsupported HDR/depth/SAR facts refuse.
  Transfer/primaries/range/matrix/chroma declarations must match as well; decoded
  RGB signal values alone do not prove identical display interpretation.
- Sound: compare the full native stereo PCM24 stream and exact sample count,
  without resampling, padding or channel conversion. Packet sample-clock gaps
  also refuse; matching concatenated PCM alone must not hide a timing defect.
- Rights/provenance: the baseline remains an exact selected/named Tig output
  under explicit definition admission. It becomes comparison evidence only.
  No new cache, baseline copy, approval transfer, selection, upload or removal.
  Reports contain private content digests and must remain local/private.

Provider boundary: borrowed protected providers hold effective source guards.
The CLI candidate remains a producer-owned local directory checked through the
existing Path provider, which is not a concurrency/custody guarantee. Full
durable job/process recovery and production adoption remain open.

Synthetic tests cover byte-different rewrapping, changed pictures/audio,
color retagging, truncation, video/audio clock gaps, stale pins and wrong rate.
The Tig CLI fixture also compares a held reference and catches a cel-B-only
change with unchanged PCM. Final local and exact-head CI results must be
recorded before merge. Actual fixture results belong in private artifact notes.

Final local tests pass: the explicit synthetic FFmpeg comparison case covers
rewrapping, image/audio changes, matrix/transfer retagging, truncation, video/audio
clock gaps, stale pins, rate mismatch and higher-depth audio refusal. The backend
target passes 15 tests with its one real-render case initially ignored; that
case was run explicitly and passed with comparison equality/mismatch assertions.
All-target/all-feature warnings-denied Clippy, formatting and whitespace checks
pass. Exact-head Windows/Linux CI remains required before merge.
