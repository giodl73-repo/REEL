# Additive tier backup

`tier-backup <request.json> <new-private-report.json>` previews custody preparation
for an exact hash set from a fresh tier plan. Add `--apply` to publish verified
copies to the configured retained tier. The request uses the existing strict
`reel.asset-demotion-request.v1` contract, positive budgets and an explicit cold
or history target. It never changes selections, registers unknowns as project
assets, removes source links or grants removal authority.

This works for both registered artifacts and unregistered objects whose exact
hash and byte count appear in the frozen inventory. `version-backup` keeps its
separate requirement for an exact version transition. Unknown hashes, stale
plans, changed source controls, invalid budgets, corrupt existing destinations
and unavailable/full targets hold the operation. Existing corrupt bytes are
never overwritten.

Publication reuses the existing guarded immutable-copy implementation: source
hash/size verification and read-only handles, target reserve admission, guarded
ancestors, atomic publication and target verification. Source bytes survive
failure or interruption. Duration is checked between objects; byte/object limits
also bound each operation. Linux supports preview; destructive/publication apply
remains Windows-only in this implementation.

After backup, use a fresh scoped `tier-demote` dry-run and explicit apply. That
separate command rechecks all consumers, registrations, selections, leases,
physical copies and exact retained custody. A successful backup alone is never
permission to remove E copies or permanently retire retained history.
