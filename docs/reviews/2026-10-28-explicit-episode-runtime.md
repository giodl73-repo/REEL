# Explicit episode runtime

Episode conform previously used ambient FFmpeg/FFprobe for its own operations,
even when upstream scenes and presentations used protected providers. The Rust
`build_with_files` entry takes `Files`: exact native scene input/output maps,
the adopted-presentation map, and an explicit episode media-program provider.
Both program selections are preflighted before component reads. A refusal
propagates without an ambient retry or successful output directory.

Every episode probe, normalization, color/geometry comparison, decoded digest,
padding operation, concat, timestamp check, cut-boundary measurement, encoder and
compact-delivery verification uses that provider. Presentation/source rechecks
keep their independently supplied providers and runtime selections. The legacy
entry points retain their local contract with exact PATH tool resolution.
No process-global PATH mutation, global tool binding or cache lock is introduced.

The existing cache-only fake episode fixture spawns the same Rust test executable
as an isolated child with PATH empty. It binds exact host/FFmpeg/FFprobe bytes
through `BoundPrograms`, prepares every primary input from the captured Git view,
opens all scene output bundles under Tig mutation protection, and assembles with
all original render directories absent. It checks 96 frames, 192,000 samples,
exact ordered decoded content and continuous timestamps. Independent refusal of
either episode tool must preserve its provider error and leave no final output.

A second child assembly uses 32-pixel declared aspect-preserving normalization,
an explicit unspecified-matrix/SAR policy checked against source RGB, lossless
H264 intermediates, rational concat clocks and compact H264/AAC delivery. It
requires independent master equality, exact frame/sample counts, full delivery
decode and retained lossless master. All data is generated and temporary; no
actual project cache or production runtime is modified.

Editor/Sound Designer review lenses require the existing ordering, aspect,
picture, PCM, clock and boundary checks to survive the runtime refactor. The
Rights and Provenance Steward lens requires exact selections and independently
bound tools, no external transfer or path serialization in native receipts, and
no technical result treated as selection, creative approval or release. These
producer checklist findings are not human approval.

This library capability does not itself capture a complete episode job context,
qualify upstream producer claims, reserve managed scratch/output capacity, enroll
complete custody, publish/select a delivered version or authorize eviction. Those
remain obligations of the full managed episode executor and project cutover.
Local qualification passes: both episode lifecycle fixtures (91.97s), all thirteen
presentation regressions (39.24s), and 205 library tests with nine explicit external
cases ignored (1.46s). Final warnings-denied locked all-target/all-feature Clippy
passes (2.61s); the final cache-only/empty-PATH fixture, including normalized compact
delivery and both explicit tool refusals, passes (81.77s). Formatting and diff checks
pass. The initial fixture compile caught a missing module import; that import was
corrected without dependency changes. Hosted exact-head Linux/Windows/FFmpeg
qualification remains required before merge.
