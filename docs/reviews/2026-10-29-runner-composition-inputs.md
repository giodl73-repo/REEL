# Explicit composition consumption through the existing job runner

Entry 8cecd2d14f1a4141ed7ec3457b5ce1c9ea6c2707, PR234 remains live and must not
be changed. Scope: optional Tig runner adapter, generated fake-episode fixture,
this review. No vendor fork, provider, registry, cache setup or runtime install.

The legacy runner treats every Evidence role as post-execution evidence. Tig now
supports pre-execution composition/evidence, which requires explicit consumption.
Add a separately named entry point that admits these roles as generic job Input,
without relabeling them Primary or Runtime. The original entry point must refuse
contexts requiring these new roles. Both entry points refuse ClaimedOutput.
Tig Claim validation requires at least one output, and JobContext is opaque and
derives ClaimedOutput from every validated claim; this is the phase discriminator.

Ownership/revision fences, exact sizes, input guards, capacity, process containment,
publication/readback, retention and cleanup remain the existing runner's duties.
Generic execution still creates no native producer claim or creative approval.
Complete episode/child-primary/presentation admission remains separate.

L0: fmt, locked all-target/all-feature Clippy and diff. L1: default refusal,
post-output refusal before manifest IO, wrong owner, wrong role and size before
staging; exact journal unchanged on refusal. L2: real generated-media remux with
composition/evidence inputs, extracted attachment equality, unchanged decoded
audio/pictures and durable unproven-result retention. Use existing D fixtures.

Producer lenses: Editor and Sound Designer require unchanged native frames/PCM;
Rights/Provenance requires explicit input evidence and separate execution versus
production/creative authority. These are checklists, not independent approval.

Producer disposition: pass_with_risk for explicit generic consumption only.
The cache-only fixture remuxes a selected scene with its admitted JSON evidence
attached, then compares every decoded frame/sample and the extracted attachment.
Wrong owner/size/role and post-output aliases refuse; refusal preserves journal
state and creates no staging directory. Successful retention keeps the source
predecessor, records no native production claim, and removes private workspace.
It runs in the cache-only fixture, independently of the other fixture's closed
delivered-version ledger; adding an unregistered generic result to that eviction
ledger would invalidate its coverage assumption.

Qualification: locked all-target/all-feature Clippy with warnings denied passes
(8.30s); both complete generated episode/cache cases pass (118.31s); the existing
managed scene/candidate/publication/runner/Git rehearsal passes (285.35s). That
rehearsal initially stopped because its explicit REEL_TEST_TIG_PROPOSE binding was
absent. Building the qualified vendored helper (12.05s) and setting the per-process
binding corrected the harness; no production assertion was bypassed or relaxed.
An earlier compile caught use of a crate-private source method in the test;
corrected using the real fixture instance/schema binding and exact record pin.
Formatting and diff checks pass. New-head hosted qualification remains required.

PR234 is still qualifying its exact source. Main rules require strict up-to-date
status checks and linear history. Preserve this tested child source on a branch;
after that parent qualifies/merges, rebase only this delta with whole-tree equality
before opening its main-bound qualification PR. Do not waste a stacked CI run that
would require a second head after the parent's squash merge.
