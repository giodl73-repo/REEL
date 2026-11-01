# Tig delivery preparation review

Entry `b1ce0b5fa8c27f0374a200d870168217761f3a1c`. Scope:
[delivery-template preparation](../tig-delivery-template.md) over the existing
native source, control selector and delivery/compiler APIs. No new provider,
renderer, cache, execution journal or production activation.

Producer-applied sound/editor/rights and storage/privacy lenses:

- Stable input aliases lower to current exact FileRefs; unchanged parent
  controls do not duplicate a leaf's changing hash. Original source snapshots
  retain old instances and regenerate their original controls.
- Control and primary definitions remain disjoint. Every catalog reference
  resolves before controls are interpreted; consumed draft inputs refuse.
  Unused alternatives remain permitted. Request admission is trusted policy,
  not independently inferred proof of a definition's media role or JSON Schema.
- Raw source FileRefs cannot bypass binding resolution. Nested timed-overlay
  inputs must also belong to the same primary catalog with exact current pins.
  Receipts are bounded metadata, not execution/custody authority.
- Small controls use explicitly admitted authored fields. Large/decimal-valued
  controls use the existing selected/stream byte selector and guarded reads.
  Selected JSON rejects duplicate keys before normalization. Tig's small-record
  canonicalizer is not silently expanded to admit large floating-point documents.
- Lowering creates no file and opens no media. Native planning, actual protected
  source/runtime/resource enrollment, render/check, retained results and ownership
  handoff remain required. No matching pin grants approval or deletion authority.

Initial fixture defects were found by existing native integrity admission:
the synthetic root/control definitions had omitted their authored property names.
The fixture declarations were corrected; the source loader was not weakened.
Private media and machine paths are absent. Hosted and actual-project acceptance
remain distinct from local synthetic tests and this producer review.

Remaining integration: managed recipe/capture/worker adapter and CLI, selected
variant semantics, actual Recuerdos timeline/VFX controls and current-delivery
correspondence. No full music-video production or CAMINOS cutover is asserted.

Local validation: seven regular integration cases pass, including selected
70 KiB controls with fractional media values and duplicate-key refusal. One
explicit native FFmpeg render/check passes with the original loose JSON controls
and Git unavailable; decoded nonzero music PCM matches its primary exactly.
All-target/all-feature warnings-denied Clippy and formatting pass. Hosted
qualification, actual project authoring and installed runtime adoption remain open.
