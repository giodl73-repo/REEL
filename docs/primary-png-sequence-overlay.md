# Ordered primary PNG sequence overlays

Entry `ea51342a28f8babe6d5bc84597fb5cabb7ec51a5`; native REEL adapter
scope under Tig WP-010/011, REQ-003/006/010 and the existing native delivery,
source/context and guarded-input contracts. Add explicit
`timed-png-sequence-overlay` with an ordered `frames` array of primary FileRefs
on an external layer. The first frame must equal its `evidence` reference.

Each entry contributes exactly one frame to the existing half-open semantic
effect/overlay span at the declared delivery rate. Require exact span length,
canvas geometry, constant pixel format/bit depth and alpha PNG content. Repeated object identities are allowed;
order and logical references remain significant. Refuse animation, derived
render sources, receipts and fonts. Other render modes must have no `frames`.
Limit total frame references to 4,096 per job; all ordinary source/read/metadata
budgets remain in force. Existing wire output omits empty arrays.

Resolve every frame through the same primary catalog and guard set. A temporary
bounded FFconcat control references existing protected files; it copies no image
payload and creates no intermediate input movie. Admit only absolute UTF-8 paths
without control characters, quote them as data, cap each control at 1 MiB and
normalize input timing to the exact delivery rate. Keep this physical-path
control ephemeral. Both that control and the retained index are capped at 1 MiB.
Published output provenance is a separate ordered logical
FileRef index, never physical provider paths. Independent primary-reference
replay uses the same bounded source admission and counts its control staging.

Normalize both new still/sequence overlay inputs to integer ticks on the exact
delivery frame grid. Rescaling only the sequence is insufficient: a base
Matroska stream can round timestamps to milliseconds and skip the intended
first or alternating frame. Sequence decoding disables filter reinitialization
after admitting constant format so the frame counter stays continuous. Existing
video-overlay clocks remain unchanged. Tests must check known source-frame
order in addition to agreement between renderer and primary-reference replay.

Entry/exit: existing guarded source and semantic attachments to the existing
checked-candidate/output/receipt contracts. L0: fmt/diff/Clippy; L1/L2: ordered
alternating PNGs over a cut, repeated identities, exact all-frame order, unusual
quoted paths, malformed count/geometry/alpha/animation/fields, template binding
expansion and protected-source/archive cases. No new cache, ownership protocol,
creative approval or production activation. Actual project enrollment remains
separate from synthetic verification.
