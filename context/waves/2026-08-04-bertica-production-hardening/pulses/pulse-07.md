# Pulse 07 — Advisory configurable asset tiering

Owned slice: `reel-assets` read-only planner and freshness checks. Reuse a
retained census with current registry/selection/lease deltas, named generic
tiers, explicit previous-use locks, bounded proposals and private output.
No public consumer data, private assets or per-project cleaner scripts.

Safety boundary: no apply/move/delete/cold census implementation. Output rows
are non-executable and carry no removal authorization. Physical observations
are baseline-only. Other current work, unknown ownership and unproven historical
use remain held. This is phase 1, not completion of a storage-reclamation tool.

Validation results are recorded in the dated handoff. Required checks: crate
synthetic tests, Clippy, workspace formatting, pulse workspace checks, ROLES
metadata and whitespace. Private consumer dry-run evidence stays upstream in
cache/LFS, separate from public synthetic verification.

Next slice: exact cold inventory/custody and verified, journaled removal of
redundant hot copies after fresh destination/source and consumer checks. Separate
last-copy retirement must not be enabled by the ordinary tiering API.
