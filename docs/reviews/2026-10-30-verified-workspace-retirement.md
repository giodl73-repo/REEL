# Verify retained originals before removing recovered scratch

Result adoption deliberately grants no deletion authority. The separate
`retire-workspace` command defaults to verification and requires `--apply` for
removal. It accepts only a completed current result and the exact original
recorded operation; dry-run and apply leave source ownership and journal events
unchanged. All retained packet objects and original request inputs/tools remain
protected through the operation. Installed tools execute in place; their
namespaces must be outside the workspace. Their current availability/hash is not
a proof required to remove unrelated historical scratch.

The implementation reuses the runner's bounded owned-workspace inspection.
Exact canonical cache/staging/job identity, non-reparse descendants, manifest
bytes and all existing declared output pins must match. Exact-job reservations
and leases refuse; unrelated writers remain independent. There is no global
maintenance lock held through verification/removal, no implicit stale release,
no archive fallback and no cache payload deletion. Logical view bytes are
reported without pretending they equal physically freed disk space.

A fast synthetic custody fixture exercises default dry-run, missing canonical
primary with a surviving workspace hard link, reservation/lease refusal,
workspace size and output mismatch, junction escape, unrelated writer presence,
explicit removal and repeated apply. It invokes the actual CLI without media
execution or native producer assertions.
Additional cases cover a missing installed tool, newer pending work on the source
and a failed job whose salvaged candidate remains retained without changing its
outcome. The source journal does not gain retirement events or lose predecessors.

Producer-applied storage/provenance checklist: retained canonical bytes precede
removal, private paths and reports stay outside Git, authority remains scoped to
one owned workspace, and technical custody conveys no creative approval. These
are checklist findings, not independent human approval. Current implementation
requires Windows read-sharing guards. Incomplete/unpublished salvage, child
handoff and actual consumer cutover remain open.
Process-interrupted partial removal is covered by a separate bounded local
transaction: apply atomically records the exact scope, original directory
identity and entry snapshot before deleting anything. Retry rechecks custody and
accepts only unchanged surviving entries; it can tolerate a missing manifest
only with that prior intent. Directory timestamps may change as children are
removed. File changes, new entries, directory replacement and corrupt/mismatched
metadata refuse. No transaction advances during dry-run. Completed records are
compacted and cannot authorize deletion of a replacement directory.

An optional exact retained-result pin permits cleanup after the source advances.
The old result must still be owner-retained at its original version, and its
packet/operation/input bindings and actual bytes must verify. A direct duplicate
input predecessor may be retired without losing the binding through the result.
Default current-only admission remains strict. The fixture advances ownership,
retires the duplicate direct input reference, checks wrong/released result pins,
and verifies that cleanup preserves the newer source and its workspace/lease.
