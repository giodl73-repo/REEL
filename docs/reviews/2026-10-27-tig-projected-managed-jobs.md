# Projected native managed execution

Entry25f956546363cd680206cf7e1b9df22737763e12 (REEL206, dependent stack).
Adapter contribution to Tig WP-010/011, IF-010/011. Scope: explicit projected
render entry points requiring held BoundPrograms, plus a separate managed
JobPlan constructor. Reuse existing context/recipe validation, scoped ownership
fences, job-resource metadata, render/check, publication and output handoff.
No new cache engine, global lock, runtime binding, daemon or CLI default change.

Mode is selected by the API, not inferred from a stored report. Legacy constructors
and rendering keep legacy context/recipe admission. Projected planning compiles
into new caller-owned preparation scratch; execution independently recompiles
into new job scratch and checks frozen parameters before opening primary media.
Require exact planned versus rendered recipe provenance before returning a managed
candidate. The operation parameter pin remains the frozen context descriptor.

L0: format/lint/default isolation. L1: projected job metadata freeze/readback,
captured controls/primary/runtime roles, cross-mode refusal, existing-directory
refusal, stale owner/revision fencing and no global job lock. L2: synthetic Windows
FFmpeg managed render/check, publication, output role separation and durable
ownership handoff followed by projected claim replay with Git unavailable.
Keep normal planning checks independent from external FFmpeg qualification;
explicitly run the latter and require exact-head CI before squash merge.

Producer editor/sound/rights lenses: preserve full native controls and exact
renderer contracts, live file guards and source ownership, independent output
checking and immutable publication evidence. A verified declared runtime manifest
is not a complete OS/environment proof. Technical checks grant no creative
selection, release, global custody or eviction authority. Full adoption and
generated-input freshness remain open. Roles are producer checklists only.

Executed evidence:47 ordinary all-feature native cases pass with7 explicitly
ignored external/source cases. The new normal Windows case freezes/readbacks
projected job resources under held non-executed runtime stand-ins, preserving
all primary/control/runtime roles and exact frozen parameters. Legacy constructor
and existing-directory execution refusal cause no further input reads/output.

The explicitly invoked new Windows FFmpeg case passes (97.30 seconds): projected
managed rendering, independent checking, verified publication, output-role
separation, owner/revision/journal refusals, peer-job progress, output root handoff,
and original projected claim replay after Git/working records disappear. It
reuses the same job-resource/runner exercise as legacy execution; the helper now
returns its checked published claim. New CI explicitly invokes this ignored case.
The existing legacy Windows native candidate/CLI/Git-handoff fixture also passes
(255.85 seconds), verifying the shared renderer/helper changes in legacy mode.
All writes/renders are inside owned synthetic temporary fixtures, not production
caches; no private retained-scene media is rendered or uploaded.

Locked all-target/all-feature Clippy with warnings denied and default library186
tests (9 existing ignored) pass, as do format/whitespace checks. No Rust production
code changed after the external render qualification. Producer editor/sound/rights
disposition: pass_with_risk for explicit managed projected execution. Actual
generated-input freshness, full runtime/custody/global coverage and CLI adoption
remain open; unsigned correspondence is still distinct from production approval.
Exact-head Linux/Windows/FFmpeg and dependent-base integration precede squash merge.

Integration progress:204's old f496223 head finished all3CI successfully before
the rebased18050ba7 branch was pushed with an explicit f496223 lease. The rebased
tree is byte-identical and now based on actual main70778d7; fresh exact-head CI is
running. Do not merge using the old head's checks.205/206 remain dependent PRs.
