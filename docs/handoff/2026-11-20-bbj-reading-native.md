# Full BBJ reading issue: intake retained and closed, admission fix required

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
parent admission. The one original `import-handoff` completed successfully;
`native-intake/import-result.json` reports Retained and technical completion.
The journal's durable third event (`00002.json`) records that same Retained
state. Its acknowledgement SHA256/length match the native acknowledgement
control. `retained-intake-observation.json` records those exact pointers without
claiming assembly, editable restoration, approval or selection.

The import output was created at 18:31:52 UTC and the durable Retained event was
written at 18:51:07 UTC on November 20: approximately 19 minutes for 88 files /
127,066,343 bytes. This measures that original operation; it is not an episode
throughput qualification or proof of one specific bottleneck. The process exited
successfully, rather than being restarted after an observation timeout.

`native-intake-close/prepare-result.json` passed the original guarded close
preview. The original `close-job` then completed successfully. `closed.json`
records applied handoff/clear work, original clear completion and an idle actor.
`intake-closed-observation.json` also checks the original intake profile against
its unchanged base-binding pin. No payloads, retained history or scratch were
deleted. The source intake is technically complete and its actor is returned.

The original `advance-recipe` exited with the definite pre-admission conflict
`composite-child-store-actor`, recorded in `advance-result.json`. Its owned
directory contains only its original intent and `state-000.json`; no pending
frozen step, child capsule, child admission or worker was created. The comparison
mistook the parent reader's 128 MiB ceiling and child's 256 MiB ceiling for a
different native store. Pool/store enrollment is identical. The consumer limits,
recipe, operation, attempt and original runtime must remain unchanged.

See [the bounded reader admission fix](2026-11-20-reader-bound-admission.md).
Observe any specifically started preparation/admission process first. Never
treat an observation timeout
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
The original runner has not adopted this step. Inspect its state before
claiming child admission, worker execution or retention. The capsule's recipe,
base bindings and runner configuration remain original.

The intake guarded close is complete. Its post-close metadata observation leaves
10,341 nodes / 2,426,322 bytes of configured headroom; it reserves no future work.
After separately preparing/accepting the exact original frozen admission through
the fixed coordinator, continue the original reading recipe invocation, using
its original base bindings and runner config. The old runner's exact-operation
recovery path can adopt that already accepted work. All source, input and worker
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
The earlier recorded observation in `metadata-capacity-observation-3.json` is
86,937 nodes / 30,209,718 bytes, leaving 13,063 nodes / 3,344,714 bytes under the
existing 100,000-node / 32 MiB limits. `metadata-after-intake-close.json` records
the later post-close counters. Neither observation is a forecast.
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
is unchanged. Native intake completion and close are established separately by
their actual evidence. These code checks do not establish child admission,
complete issue retention/review/fix or S5 production readiness.
