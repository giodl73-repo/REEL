# Compiled-output placement across worktrees

A branch's latest metadata record is not, by itself, a demand to keep every old
compiled movie on the hot tier. An explicit tier policy may reference a private
cache/LFS asset in its optional `output_retention` descriptor (`sha256`, `bytes`).
This is placement authority, not a build-input map, creative approval or a claim
that every legacy artifact has migrated.

The asset uses `reel.output-retention-map.v1`, the policy's `project_id`, a
`consumer_selections` object and an `outputs` object keyed by complete SHA-256.
Each output declares `bytes`, `selected` and `archive_authorized`. Classify only
known compiled outputs from producing records; do not infer them from age or a
filename. Unknown inputs remain held. A classified primary-input conflict fails.

Generate an ordinary `consumer-protections` export before creating the map.
Its consumer rows provide canonical root keys and `output_selection_sha256`
values for `consumer_selections`. These freeze current version pointers and
the existing authoring/selection JSON. New authorization rows or job leases do
not invalidate that selection snapshot. A changed selection, missing consumer,
foreign project, corrupt asset or missing active control holds maintenance.
Foreign legacy repositories cannot inherit another repository's output policy.

Only `selected: false, archive_authorized: true` waives an authoring-head hold.
The resulting plan class is `archival-output`, with an explicit cold target and
`verify-and-archive-output` proposal. This makes no assertion of actual previous
use. Selected outputs, primary inputs, active leases, tier pins and registry
controls retain their protections. Ordinary historical/source references and
immutable version records remain untouched.

An exact bounded `tier-demote` request may set `archive_outputs: true` and name
the policy's cold tier in `retained_tier`. It requires the fresh matching plan,
complete managed-consumer scope, exact retained custody, dry-run and explicit
apply. Every requested hash must have the explicit archival disposition. A
render-version transition is unnecessary for this placement-only operation.
`allow_live_admissions` may coexist with unrelated producers; exact leases,
selection snapshots, source controls and file/link identities are rechecked.
No daemon, automatic reclamation, media rebuild or worktree hydration occurs.

The cache census continues to report metadata usage conservatively. The tier
plan overlays this explicit placement decision. It does not mark global source
coverage complete or silently classify other current records. Existing projects
without an output-retention map keep their previous behavior.
