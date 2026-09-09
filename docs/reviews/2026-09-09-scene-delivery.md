# Compiled scene delivery review

Scope: scene-delivery plan/render/check and consumer adoption contract.
These are AI-operated repository role checklists, not creative approvals.

| Lens | Finding | Disposition |
| --- | --- | --- |
| Editor | Two independent timing implementations can drift after a recast. | Renderer consumes compiler attachments; one global frame partition and decoded frame test cover the join. |
| Editor | Renaming the same picture can disguise an unchanged long composition. | Consecutive equal source/crop spans accumulate; explicit stillness decision references are required over the consumer limit. Human review still assesses near-identical crops. |
| Sound designer | A missing music bus can masquerade as a finished mix. | Exactly D/M/E policies, positive consumption evidence, held-state failure, native cue coverage, sample-addressed delays and full-length role outputs. |
| Sound designer | Loud sums can clip while file/geometry tests pass. | Float roles and mix are inspected before PCM24 quantization; overload fails. Existing audio-check and listening remain necessary for true peak, loudness and intelligibility. |
| Animation director | A scene bridge should not replace existing motion/VFX adapters. | Still crops and existing motion clips are supported. External camera/effect/title/caption evidence is explicit and never certified as rendered by this receipt. Consumer must run existing checks and inspect final inclusion. |
| Platform/audience | Repeated review encoding reduces picture and sound quality. | FFV1/PCM master, separate one-generation H.264/AAC review. Consumer episode conform must use masters, not reviews. |
| Rights/provenance | A render must not become selection or publication authority. | Hash/byte verification, local-only paths, no network/synthesis, immutable new output directory, technical-only receipt and synthetic fixtures. |

Adoption condition: one controlled real-scene parity proof plus a native-recast
selective-rebuild proof before wholesale migration. Unresolved external layers,
creative choices and performance issues remain consumer-owned. Do not call this a
new effect engine, a complete episode renderer, an automatic emotion system, or a
replacement for the existing changed-only graph.

## Local verification

- cargo fmt --all -- --check: PASS.
- cargo clippy --all-targets --all-features -- -D warnings: PASS.
- cargo test --all-targets --all-features: PASS; platform/tool-dependent ignored tests remain explicitly ignored.
- Final focused scene-delivery suite including real FFmpeg: 7/7 PASS. It additionally tests exact boundary pixels, overload rejection before publication, case mismatch and traversal.
- Untimed production fixture validation, repository ROLES and git diff --check: PASS.
- CI executes the real scene-delivery FFmpeg canary on both Linux and Windows. Remote results remain a merge prerequisite.
