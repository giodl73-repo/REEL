# Remove a source-checkout dependency from review ID allocation

Entry: REEL main `57e4ca590ccf7062f9a0c3e706e068da7a1ee240`.
Scope: a generic library/explicit CLI in reel-assets and synthetic fixtures.
No private source table, profile, source hash or provider path is copied here.
No cache/renderer/production binding or upstream Tig source changes.

The final consumer intake exposed a mutable label registry in an old writing
worktree. Retained snapshots alone cannot provide a single live allocator for
multiple new worktrees. This implementation imports an exact pinned baseline,
preserves its bytes/labels/counters, and records only new assignments in a
durable chained append journal under a bounded per-registry lock. No global
cache lock, daemon, timestamp selection or automatic counter reset is involved.

The eight focused cases cover source-unavailable replay/idempotent retry, late
invalid input and 513-source refusal before append, invalid/duplicate-key and
regressed-counter imports, truncated/corrupt/repeated/blank journal refusal,
twelve concurrent workers, baseline tampering and reimport refusal, actual CLI
report preflight and privacy, and eight independent concurrent CLI processes.
All pass on Windows. The first concurrency run found that Windows lock contention
does not necessarily map to ErrorKind::WouldBlock; matching fs2's platform
contention code fixes the bounded retry without changing lock scope or timeout.

Full reel-assets package tests and strict all-target/all-feature workspace
Clippy pass. Full workspace and hosted exact-head qualification remain pending
until their results are recorded. This feature changes no media semantics.

Producer-applied rights/provenance and editor checks: review labels identify
source bytes and never confer selection, approval, spoken-text or release rights.
Private reports include source identities and belong in asset storage. The
explicit legacy schema is an import adapter, not a generic engine dependency on
a project checkout. Kinds/counters are data-defined; no family/drive is hardcoded.

Before a real consumer switch, stop the old allocator, verify the final source
pin, import into a new shared state location and compare every original mapping.
Then route every resumed producer to that one authority and retain a recovery
snapshot. Never run old and new allocators independently. Live adoption, qualified
versioned installation and consumer-profile acceptance remain separate work.
