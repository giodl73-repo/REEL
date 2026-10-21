# Advisory asset tier planner checkpoint

Implemented in the existing `reel-assets` crate: explicit `tier-plan` and
`tier-plan-check` commands, configurable named tiers, retained census identity,
live authorization/version/selection/lease reconciliation, explicit historical
use locks, bounded proposals, plan expiry/integrity and stale-state rejection.
The existing accounting graph helpers are shared rather than copied.

Validation at this checkpoint: 55 asset-crate tests pass, including 12 new
tiering cases; crate Clippy with warnings denied and release build pass.
Workspace formatting, Clippy with warnings denied, role metadata and whitespace
pass. The full workspace test command is still running its existing media
integration fixtures; its final outcome will be recorded separately before
claiming that gate complete. Private consumer execution and real scoped LFS
restoration evidence remain upstream, never in this public repository.

Producer-applied rights/provenance review: public tests use isolated synthetic
roots/data; no private inputs, paths, hashes, permissions or media were imported.
Metadata/technical success grants no creative or publication approval.

This checkpoint authorizes no reclamation. There is no move, apply, cold-census
or last-copy retirement API. Every output is explicitly non-executable; archive
roots are not probed and physical observations remain baseline-only. Current
working inputs, unknown ownership and unproven historical use are held.

Next owned unit: verified and journaled removal of redundant historical hot
copies already present on the configured history tier. Require fresh hash/bytes,
physical alias and protection/consumer checks, capacity where copying is needed,
safe handle/path operations, stale-state rejection and interruption/restart
tests, then a bounded real pilot. Cold registration and permanent retirement
remain separately gated follow-ups.
