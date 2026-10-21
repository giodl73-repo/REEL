# Project-configured asset management

REEL owns the Rust engine and offline review UI. Each producing project owns its
catalogue, immutable version locks, creative selections, tier policy and private
reports. There is no required drive letter, project title, server or daemon.

## Configure a project

Start with the three JSON files in
[`manifests/templates/asset-manager`](../manifests/templates/asset-manager).
Copy `asset-project.json` and `asset-manager.json` to the project root and
`cache-policy.json` to `operations/`. Set the same project ID in the profile and
policy. Declare every registered Git worktree branch active or inactive;
undeclared and detached consumers hold the review. Separately maintained repos
sharing the caches must also be managed: use the explicit all-consumer scope in
[tier review](asset-tier-review-v1.md). Git worktree discovery alone cannot find
unrelated repositories.

Create the ignored `.local/storage-rust.json` with **your own absolute local
paths**. For example, replace these paths with existing storage on your host:

```json
{
  "path_policy": "configured-local-roots",
  "cache_roots": ["/mnt/working/project-cache", "/mnt/history/project-cache"],
  "archive_roots": ["/mnt/archive/project-cache"],
  "coordination_root": "/mnt/working/project-cache/control",
  "capacity": {
    "minimum_free_bytes": 10737418240,
    "minimum_free_basis_points": 500
  }
}
```

Windows paths such as `X:/project-cache` work as well. The array positions bind
to policy tier indexes; tier IDs are project choices. Active tiers must be on
independent volumes for a tier plan. Archive roots may remain offline; reviews
do not scan them. Local storage and all report directories belong in `.gitignore`.
Omitting `path_policy` preserves the legacy consumer's G/M restrictions;
`configured-local-roots` explicitly uses the configured tier roles instead.
Both modes reject relative and network cache roots.

Use the normal registered-object and version contracts. A new, empty project
needs `catalog.json` containing `{"videos":[]}` (the compatibility authorization
channel), and `registry/versions.json` containing:

```json
{"generation":0,"versions":{},"current":{},"pins":[],"leases":{},"coverage_complete":false}
```

Keep coverage incomplete until all consumers and dependencies are accounted for.
The template's empty selection list is a starting point, not a claim that no
assets are needed. Configure selectors for your actual artifact/scene catalogues
as described in [project accounting](asset-project-v1.md). The version registry's
current roots remain protected even without output selectors.

## Register, select and retain versions

Use `ingest` to hash and publish source bytes, then `register` with their exact
hashes, sizes, provenance, rights and selection state. Registration alone does
not establish previous creative use. Use `register-version` with immutable
manifest/payload hashes and exact dependency locks; `select` changes the current
version. `deprecate` prevents future selection without deleting bytes. Shared
selected dependencies stay protected even when another referencing version is
deprecated. See [version indexing](asset-version-index-v2.md).

Previous Goldens or actually used versions enter the policy's `history_versions`
as exact `{logical_id, version_id, manifest_sha256}` locks. Age, registration and
deprecation alone never establish previous use. Registered but unproven material
remains a visible disposition gap. The engine does not infer creative approval.

## Refresh and review

Build once in REEL; run the executable with the producing project's root:

```text
cargo build -p reel-assets --release --locked --bins
reel-assets --root <project> manager-review asset-manager.json <unique-name> --refresh-inventory
```

This explicitly measures the configured active CAS and named LFS namespaces,
then freezes a new inventory, a policy copy with its exact baseline hash, a plan,
an empty consumer scope and `review.html` under
`.local/asset-reviews/review-<unique-name>/`. The tracked policy/declaration and
payloads are untouched. The zero baseline in the starter policy is a placeholder
for this explicit refresh; it is not a valid retained inventory.

Open `review.html` normally. Browse classifications, tier observations, version
reference labels, bounded hash previews and hold reasons. Filter by tier, sort
by size/reclaim estimate, inspect blocks and review chosen blocks. Classification
bars measure logical copies; physical reclaim may be unknown because of hard
links. Free capacity is measured at census time, not continuously. A displayed
copy is not verified recovery proof. Cold capacity remains unknown.

The optional declaration `inventory` selects an already registered cache asset
by exact `sha256`, `bytes` and `cache_uri`. Omitting `--refresh-inventory` uses
that saved asset and reconciles current registered metadata; it does not rescan
unregistered additions or removed files. With no saved inventory, refresh is
required. Opening HTML never runs an inventory, uploads data or changes caches.
Concurrent metadata changes may reject a census; retry explicitly with a new
name. The filesystem census remains observational, not an atomic snapshot.

## Choose, dry-run, apply and inspect

Save a block choice in the UI. It contains exact block/review/plan identities;
there is no browser filesystem-removal endpoint. Then run:

```text
reel-assets --root <project> tier-request <review-directory>/scope.json <saved-choice.json> <new-request.json>
reel-assets --root <project> tier-demote <new-request.json> <absolute-new-dry-run-journal>
```

Inspect the native result and per-object journal: retained verification,
protection/hold reasons and source links. Only after reviewing that exact request:

```text
reel-assets --root <project> tier-demote <new-request.json> <absolute-new-apply-journal> --apply
```

Apply rechecks current protections, independent copies and payload hashes before
each removal. Interrupted removal resumes through the same owned apply journal;
never replace its records. Fresh changes can invalidate a saved choice or hold
an action. The existing Windows handle-based apply implementation is required;
other platforms can inventory/review/verify but do not gain an unlink fallback.
Historical removal retains its legacy job-admission gate; concurrent producers
use independent metadata domains and short capacity transactions.

Copy-first movement, packed archive recovery, permanent last-copy retirement
and cloud tiers are separate capabilities. Unregistered archive-first candidates
are visible but cannot enter this browser's history-removal request.

## Retain and restore

Treat inventory JSON, HTML, decisions, requests, dry-run results and apply journals
as **private assets**. Before handoff, register retained reports/packages in the
producer's cache/LFS and keep only compact hash/byte references in Git. Retaining
them changes metadata: regenerate a review afterward rather than applying an old
choice. Temporary `.local` paths alone are not a completed asset handoff.

Use the normal explicit `resolve` operation for a registered hash or
`golden-restore` for an exact historical Golden. Verify recovered bytes before
consumption. Routine review does not hydrate an archive or retarget a selection.
After any cache action, generate a new census to see the new physical state.

## Verification

```text
cargo test -p reel-assets --all-targets --locked
cargo clippy -p reel-assets --all-targets --locked -- -D warnings
node --test crates/reel-assets/tests/tier_review_ui.test.cjs
```

Node runs development-only tests of the shipped browser script; it is not a
runtime requirement. The standalone HTML has no CDN, remote fonts, telemetry,
network access or daemon. Reports and private project fixtures never belong in
the public REEL repository.
