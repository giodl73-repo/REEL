# Full BBJ reading issue: native operation in progress

Continue the complete issue from
`F:/src/tig-dev-temp/reel-bbj-reading-issue-20261120-v1`.
The local 28-page PDF and exact pixel evidence are described in
[the reading issue report](2026-11-20-bbj-reading-issue.md).

## Exact current intake

- Task: `bbj-cp02-reading-issue-v1-intake`.
- Task key: `143431107dbe6405f54b55b04f972a9ebe4fc04b1ce8c90f8582897255773ec9`.
- Attempt: `source-intake-attempt-1`.
- Operation: `bbj-cp02-reading-issue-v1-intake`.
- Owned controls/journal: `native-intake/` beneath that root.
- Exact executable: the frozen native runtime whose SHA256 is
  `a62c825756d1be63374d74627855a275c53ec49c628889e960f26aa8d0181070`.

`native-intake/preview-result.json` passed the actual live slot preview.
`accept-result.json` records verified durable input protection and empty-root
parent admission. `create-result.json` records Waiting. The one original
`import-handoff` invocation is running, with its output directed to
`native-intake/import-result.json`. It began at approximately 10:31:52 local
time on November 20. Last observation was still live, with changing native
Git-annex child processes. The journal contains the original Waiting and
ImportPending events. No retained acknowledgement has been observed yet.

Observe that original process/session first. Never treat an observation timeout
as termination, restart under a new identity, alter the original profile/runtime,
cancel an ImportPending job, or discard scratch. If it is terminal, inspect its
output, original journal and acknowledgement directory before choosing the
documented same-operation recovery path. Preserve every response/publication
handle on uncertainty.

## Next operations after verified retention

The immutable `base-bindings.json` pins this intake profile. `runner-config.json`
declares the future `bbj-cp02-reading-issue-v1-compose` operation with attempt
`reading-compose-attempt-1`, the same frozen lifecycle runtime and an explicit
272 MiB total / 256 MiB object return envelope. Its worker workspace is 1 GiB.
It has not yet been adopted or executed. The capsule's recipe remains original.

Once the intake is Retained, prepare and execute its guarded close into the
original retained target. Recheck metadata headroom and close success before
reusing the owned actor for assembly. Then advance the original reading recipe,
using its original base bindings and runner config. All source, input and worker
directories are specific to this root; earlier jobs and other actors stay intact.

After assembly retention, use the new Rust `plan-comic-review` command against
the exact resulting plan/producer packet/result/upstream declaration. It emits
the next protected review-control recipe, carrying original source selectors,
the producer outputs and its worker receipt. Owned native execution must still
verify retention; the planner grants no custody, viewing or approval.

## Read-only capacity observations

The new `inspect-metadata-capacity ADMISSION_PROFILE` command uses the qualified
SDK `MetadataReader`, rather than opening a metadata writer or scanning media.
It verifies identity/minimum checkpoint and reports configured limits/current
counters/headroom, without reserving future work or changing capacity.
The latest recorded observation in `metadata-capacity-observation-3.json` is
86,937 nodes / 30,209,718 bytes, leaving 13,063 nodes / 3,344,714 bytes under the
existing 100,000-node / 32 MiB limits. That observation is not a forecast.
Do not reset/recreate a store or silently split work to bypass its limits.

The separately frozen read-only inspector is pinned by
`F:/src/tig-dev-temp/reel-metadata-inspector-tool-v2.json`. It is not the intake's
lifecycle runtime and must not replace it. A real foreign-identity observation
refuses `Conflict/metadata-capacity-foreign`; its original earlier generic
diagnostic remains preserved separately.

## Code verification

The producer-bound review planner tests check the exact baseline, original
image references and worker-receipt selector, alongside deterministic full
reading review-control generation. All 155 core workflow tests pass. Strict
core comic/default and native film/default Clippy checks pass. The SDK source
is unchanged. These checks do not establish this still-running intake's native
completion, a full issue review/fix, or S5 production readiness.
