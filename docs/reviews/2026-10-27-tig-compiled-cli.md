# Compiled-context CLI workflow

Entry219d4c1eda7491bbb434159541dfaa49d0ff3b4f (REEL208 dependent stack).
Adapter contribution to Tig WP-010/011, IF-010/011. Scope: explicit compiled
context mode in native worker admission, default legacy preserved; coordinated
preparation reservation, bounded owned scratch and projected plan/render routing.
Reuse existing capacity ledger, provider, owner/job and worker ports. No new
cache engine, daemon, global render lock or installed runtime switch.

Reserve before coordinator preparation writes. Account for two coordinator and
three worker compilation passes,64 MiB logical/file-count bounds per pass and
filesystem data-allocation rounding. Physical administrative overhead remains
covered by configured free-space policy, not a filesystem quota guarantee.
Delete only owned preparation scratch before releasing its reservation; failed
cleanup retains conservative capacity for recovery. A crash keeps the reservation
record with an unheld lease and an identifiable owned preparation directory.
Worker preparation is inside already-admitted job scratch; insufficient declared
scratch must fail before job/source-journal changes. Never infer mode from reports.

L0: fmt/lint/default isolation. L1: old/default and explicit mode intake, unknown
mode refusal, document/count/total limits, checked peak arithmetic, reservation
and cleanup behavior, insufficient scratch before owner transitions. L2: actual
synthetic CLI plan/render/record/export/handoff under compiled context with source
Git offline; recipe identity and output roles remain strict. Retain legacy tests.
Exact-head CI and ancestor integration precede squash merge. Editor/sound/rights
producer lenses preserve native meaning/runtime guards and private metadata;
technical execution is not approval, full environment, global custody or eviction.

## Producer checklist findings

Editor and Sound Designer: the explicit compiled path reuses the native scene
compiler, language-scoped recipe and renderer, with the same complete output
contract and independent media checks. It does not introduce timing, cut, mix or
translation transformations. Actual fixture execution remains the acceptance
evidence; mode selection alone does not establish media parity or generated-take
freshness.

Rights and Provenance Steward: preparation-budget is observation-only and does
not execute tools, read private scene bodies, upload or select media. Execution
profiles and returned diagnostics remain private; reservation IDs are local
recovery identifiers. Runtime declarations remain incomplete-environment claims.
No technical result grants publication, voice consent or creative approval.

Concurrency/capacity review: use the existing short capacity-ledger coordination
and per-job ownership; unrelated render sessions do not share a global lock.
Delete only the TempDir owned by this preparation. Failed cleanup keeps its
reservation but releases the process lease so explicit recovery is possible.
The allocation estimate is conservative data accounting, not a filesystem quota;
physical administrative overhead and complete crash recovery remain limitations.
These are producer checklist findings, not independent human approvals.

## Executed workflow

The explicit Windows/FFmpeg compiled CLI fixture passes in 347.29 seconds with
the final source: coordinator preparation admission, source-Git-offline replay,
contained same-binary planning/rendering, checked output retention, record/claim
export and exact Git handoff. The observation-only budget command creates no
admissions. Too-small scratch/capacity and unknown context mode refuse before
owner-journal transitions; owned preparation directories are gone at completion.
The helper restores source Git for its final proposal/application checks.
This is a synthetic scene lifecycle, not a complete private episode, all-family
adoption, global claim coverage or cache eviction acceptance.
