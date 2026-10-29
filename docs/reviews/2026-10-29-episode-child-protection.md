# Compound native scene input ownership

Entry b6915e860c743bc9ce8bbb9ece3f424d555bec1c. Scope: native episode consumer,
minimal crate-private scene-job capabilities, generated cache-only episode fixture
and this review. Preserve the live PR234 head. No new Tig/provider/cache/journal,
global cache lock, native approval or automatic release/removal.

The assembly-owned context has no child-primary closure. Combine existing native
job readbacks and per-source ownership guards instead of unioning scene profiles:
scenes can have different required control slots. Validate complete scene coverage,
exact current recipe/binding/admission/checkpoint, distinct source identities and
bounded deduplicated object/byte totals before any owner adoption. Actual byte
guards, current presentation/generated-input eligibility and parent context remain
separate. Grouped source protection alone grants no execution/delivery approval.

Adoption is per source, not batch-atomic. A late conflict leaves already adopted
roots durably protected; dropping guards releases fences, not source obligations.
Exact retries use existing journal idempotency. Verify before and after an explicit
callback; changed owner/revision/selection/source refuses its result. No long SQL
transaction or global lock spans consumption. Release/retirement stays explicit.

L0: fmt, locked all-target/all-feature Clippy, diff. L1: missing/wrong/duplicate
admissions and object/byte limits refuse without journal changes; shared primary
objects deduplicate; unrelated sources can proceed. L2: durable reopen, cache-only
native episode assembly inside grouped guards, late adoption failure/retry without
losing a peer, and stale callbacks refused before/after work. Source ownership is
not global claim completeness, payload custody or branch-tip discovery.

Producer Editor/Sound/Rights review requires exact native assembly media checks,
explicit partial-failure protection, and no promotion to production/creative
authority. These are checklists, not independent or human approval.

Producer disposition: pass_with_risk for compound scene input ownership. The
cache-only fixture now assembles its actual native episode inside grouped guards,
using independently held cached-output and native primary-input byte providers.
Shared primary bytes deduplicate. An unrelated source proceeds while all child
fences are held; the same source refuses. Missing/wrong/duplicate admissions and
object/byte limits change no journal state. A late foreign owner conflict retains
all three earlier adoptions; retry preserves their revisions and never steals the
foreign source. A withdrawal during a callback invalidates its result and keeps
the predecessor protected; a later stale callback never runs.

The callback is trusted and is not a transaction over its side effects. Partial
artifacts are not rolled back or silently deleted. A complete controller must keep
candidate production separate from delivery/promotion, and explicitly reconcile
every source/partial outcome. Dropping this group does not grant retirement or
release durable source roots. Assembly ownership, current presentation and
generated-input eligibility, complete claim/custody coverage and branch-tip/Git
adoption remain separate. No new source/provider promises replace those gates.

Qualification: locked all-target/all-feature Clippy with warnings denied passes
(47.81s); both complete generated episode/cache cases pass (140.59s), including
native assembly, partial adoption/retry, peer protection and stale callbacks;
all thirteen episode tests pass (35.17s). Formatting and diff checks pass.
The first multi-file fixture patch refused a stale formatted anchor atomically;
reapplied against the actual source before running checks. No assertion/invariant
was relaxed. New-head hosted Windows/Linux/FFmpeg remains required.
