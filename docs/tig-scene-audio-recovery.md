# Recover an original native audio stem

`reel-tig-job recover-scene-audio PROFILE NEW_ABSOLUTE_WAV` reconstructs a missing
original `D.wav`, `M.wav`, `E.wav` or `mix.wav` from an exact retained native job
and its original scene receipt. It uses the existing native mixer, with the
original recordings, placements, channel mapping, gains and fades. It opens
audio inputs only, even when the retained job also declares unavailable pictures.

The private JSON profile has schema
`reel.tig-scene-audio-recovery.experimental-1` and these fields:

| Field | Meaning |
| --- | --- |
| `byte_source` | Existing Tig byte-reader profile, requiring `prevent-mutation` |
| `job`, `receipt` | Exact SHA-256/byte-count objects for original native controls |
| `target` | Independently declared original stem object; must match the receipt |
| `output_name` | One of `D.wav`, `M.wav`, `E.wav`, `mix.wav` |
| `max_workspace_bytes` | Conservative bound: `56 * duration_samples + target.bytes + 1048576`, at most 4 GiB |
| `runtime` | Explicit host, FFmpeg, FFprobe and dependency bindings |

The output parent must already exist. The command checks the job/receipt lineage
and audio spans, holds verified source guards through native mixing, then accepts
only the complete original target hash and size. Different output from a newer
runtime is a refusal, not an approximate recovery. An existing output is never
overwritten, including at the final atomic no-clobber persistence step.

Scratch contains the native mixer buses and is scoped to this call. Only the
verified requested stem is saved. Cache publication remains a separate explicit
existing-publisher operation. The command initializes no stores, opens no images,
changes no asset selection and grants no deletion authority.

The job's `max_composition_samples` limits unchanged picture compositions;
it does not cap full scene audio duration. Audio recovery retains the explicit
workspace budget, checked source spans and exact original output hash gate.

This is forensic byte recovery. A frozen job and receipt do not become semantic
authoring, a new producer attestation, mix approval, or complete-scene validation.
Reusing original performances grants no new voice identity, consent or release.
