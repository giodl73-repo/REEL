# Native output contracts and bounded planning jobs

Entry25ee99a, qualified198/mainb8ca297, upstreamfc2a9dd. WP-004/006/008/009/010/011/012,
REQ-001/002/003/004/006/007/008/009/010, SPEC-003/004/005/007/008/010/011/012,
IF-002/004/005/008/010/011/012, PKG-003/004/006/008. Scope: one renderer-owned
output-name contract, optional derived output-budget policy and bounded native
planning artifact/CLI flow. Existing caches/providers/runner/journal only.

Extract the output set from the native renderer/checker into one pure helper
over a validated Job/Plan. Include actual overlays/fonts/camera/Motioncraft outputs;
receipt remains a separate control output. Renderer and checker consume that
same helper. No project-specific filename list in Tig. Raw PCM/RGB sizes are
advisory uncompressed data quantities, not physical allocation or compressed-size
guarantees. Actual admission uses explicit positive maxima and checked sums.

Planning must use the existing contained worker because native plan_with_inputs
verifies primary inputs and may invoke probes. Produce a bounded planning.json
artifact with exact context/recipe/producer/output names and derived maxima.
Run it with existing planned resource ownership, capacity/time/scratch admission,
verified CAS retention and owned handoff. No scratch outside that runner.
plan-outputs is explicitly a retained planning job, not a read-only/dry-run claim.

When the user supplies a budget policy instead of an output map, Run first
retains the plan, verifies its exact bytes and owned metadata root, then constructs
the render request. Re-adopt the same planned input root at the exact resulting
revision, preserving planning/input predecessors. Each phase uses source fences;
changed ownership/revision refuses. Worker independently recomputes the output
set before rendering; an extra/missing declaration must not spend a full render.
Both lists must never be editable simultaneously. No native producer claim,
approval, global coverage, archive custody or cache eviction follows.

Allow an explicit bounded runner output-count limit up to512, default64 for
existing callers, so native evidence frames are not silently truncated. Retain
existing byte, path, log/evidence, tree and workspace bounds. Check aggregate
atomic output allowance with overflow protection before the rendering phase.

L0 fmt, locked donor/native tests, warnings-denied Clippy/feature isolation.
L1 names for overlays/camera/fonts/Motioncraft, invalid counts/policies/overrides,
overflow, stale/foreign plan and request bounds, no dual editable output map.
L2 actual native retained planning artifact and auto-derived render with Git
offline, exact source transitions/predecessors, no media in planning outputs,
early mismatch refusal, existing explicit-output compatibility and scratch empty.
Rights/Provenance, Editor, identity/concurrency/provider/custody producer lenses;
no independent/human approval. Qualify/merge parent198 before final-head consumer
qualification and squash merge. Full readiness remains docs/CAMINOS-READINESS.md.

## Implementation and executed scope

Renderer and checker use one output_contract helper. The existing platform font
path representation remains unchanged. Compiled native rendering can require the
same exact contract before encoding while reusing its original prepared input
guards; there is no second media view or compile/render implementation.

The optional budget policy replaces the editable output list, never accompanies
it. Planning runs in the existing contained worker and publishes a bounded report.
The coordinator checks the current completed owner transition before report IO,
then its exact CAS bytes, typed context/recipe/producer/policy and retained tree/
header. Rendering re-adopts the original planned input root at the exact next
revision. Planning and input predecessors remain retained. Phase operation keys
are scoped, explicit and distinct from new attempts; there is no timeout reset.

Executed Windows synthetic fixture passes manual compatibility, mismatch refusal
before media (zero candidates), owned plan-only output with no media files, and
automatic plan -> verified report -> render. Git/records/definitions are physically
offline during the CLI calls. The planning report matches the actual native
receipt's full filenames and clocks; retained bytes/maxima, atomic-cap sums,
revision4/predecessor1 versus revision8/predecessors3, and empty scratch all pass.
Final capped-reservation run with explicit FFmpeg8.1 passes268.89s; the earlier
owned-guard run passes250.68s. A prior observed-runtime run with
nightly N-126390 passes276.92s. No existing private media or caches are involved.

Full donor all-target suite passes225 library cases plus binary/integration
targets. Four CLI unit cases pass ticket substitution refusal, unknown/zero/
oversized policies and checked-sum/multiply overflow, plus foreign/stale/pending/
aborted owned-result refusal, plus capped planning reservation/ceiling checks.
All27 normal consumer cases pass. All26 normal
scene-delivery cases pass, including actual ASS/timed overlays and post-camera
checks;14 explicit native cases remain ignored by that normal command. The
Motioncraft output-name/raw-size/overflow contract case also passes separately.

## Producer role findings

Method: producer checklist review against REEL Rights/Provenance and Editor plus
Tig placement/storage/concurrency/provider roles, specs/MODEL.md, specs/SAFETY.md
and docs/scenarios/FOUNDATION.md. Target: this diff's output contract, planning
module, CLI phase coordinator and output-count admission. No independent reviewer
or human acceptance is asserted.

| Lens | Finding / disposition | Owner / remaining work |
| --- | --- | --- |
| Placement | Scoped pass; explicit maxima and overflow/count refusal, advisory raw sizes separated from physical allocation | Review finding addressed and tested: planning caps metadata scratch at64MiB and reserves controls/report under the ceiling; native fixture uses128MiB render scratch and verifies planning is smaller while rendering keeps its full budget |
| Storage/custody | Scoped pass for owned disposable scratch only; positive report/output retention is not independent custody or cache-removal permission | Tig full claims/custody work remains open; incomplete coverage cannot mean zero demand |
| Concurrency/recovery | Scoped pass; per-source fences and exact revisions, current-version checks before IO, predecessors survive both phases | Owner crash-recovery/canonical handoff remain open; no reset or global render lock |
| Provider/privacy | Scoped pass; report paths come from an internal completed runner result and bytes/tree/header must agree; private profiles/evidence remain asset-side | Complete environment/provider/custody guarantees are not inferred |
| Editing/adoption | Scoped pass; one renderer-owned list and budgets instead of authored filename duplication | No source-record update, claim export, delivery approval or consumer cutover; full-family/schema/variant/adoption work remains open |

Overall pass_with_risk for this bounded native planning seam. Full-system safety,
global claims/capacity/compaction, generic result interpretation, native claim
export, independent custody/eviction and complete CAMINOS readiness stay open.
Hosted final-head qualification is still required before merging this package.
Final warnings-denied all-target/all-feature Clippy and fmt pass after the capped
reservation amendment. Donor Clippy and default/read-only feature checks passed
on their unchanged paths before that optional-publication-only amendment.
