# Shared review ID registry

Scope: source-checkout-independent allocation of stable human review labels.
Project instances stay outside Git and cache payload directories. This service
does not select assets, approve pictures, change source text or manage storage.
Kinds are project-defined uppercase codes, not hardcoded episode/media families.

Adopt an exact SHA-256/length-bound legacy snapshot into a NEW explicit state
directory. Retain the snapshot unchanged as an immutable baseline. Preserve all
source-kind/hash assignments and counters, and reject duplicate IDs, duplicate
JSON keys, unknown fields, invalid hashes or counters behind assigned IDs. The
legacy `caminos.private-review-short-ids.v1` schema is an explicit import adapter.
Historical padding remains exact, including mixed widths. Two spellings of the
same kind/numeric ID cannot identify different sources. New labels use at least
four decimal digits; the existing spelling always wins for an existing source.
New native snapshots use `reel.review-short-ids.v1` with the same declared fields.

Each allocation rereads the baseline and append journal under a per-registry
bounded lock. A batch validates completely before one durable append; existing
sources reuse their exact labels. New assignments advance only the corresponding
counter and preserve padding. The returned checkpoint covers the baseline and
every complete journal line. Partial/corrupt journal tails refuse; no automatic
truncate, counter reset or history replacement. Exact retry after a committed
append returns the same assignments. No global cache lock or daemon is involved.

State files and private reports include source hashes; keep them outside Git.
The manifest pins the immutable baseline. Adopt publishes that manifest last;
an incomplete directory is not a usable registry. Report-output failure after a
successful append is recoverable by replaying the identical request.

The old writer must be stopped before authority changes. Copying a snapshot
alone does not retire that writer. Record the accepted shared state location in
every migrated producer profile and preserve the old table as historical recovery
data. No source checkout should be required for subsequent inspect/allocation.

CLI (explicit operations only):

```text
reel-review-ids adopt SOURCE SHA256 BYTES NEW_STATE_DIRECTORY
reel-review-ids inspect STATE_DIRECTORY NEW_PRIVATE_REPORT
reel-review-ids allocate STATE_DIRECTORY REQUEST NEW_PRIVATE_REPORT
```

Request: `{"schema":"reel.review-id-request.v1","sources":[{"kind":"C","sha256":"<64 lowercase hex>"}]}`.
Maximum 512 sources per batch. A missing source kind refuses rather than creating
an unreviewed counter namespace. Exact duplicate requests share their one label.
Read-only inspection returns the complete recovered map/counters/checkpoint.
The CLI writes reports without overwrite and prints counts only. Import/inspect
does not allocate new IDs, hydrate media or start another process.
