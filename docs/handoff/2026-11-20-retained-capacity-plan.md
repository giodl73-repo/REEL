# Immutable retained-owner capacity recovery plans

The [actual capacity diagnosis](2026-11-20-retained-owner-capacity.md) identifies a
cumulative-owner limit, not a failed render or oversized job input. The native
adapter now exports and rechecks an immutable proposal for a separate, finite
owner-history allowance. Planning is read-only. The separate staging adapter
preserves the original retention identity and leaves work execution with the
frozen original runtime.

`retention::capacity::{prepare,recheck}` and these CLI commands expose the boundary:

```text
reel-workflow-native plan-retention-capacity PACKET SOURCE ADMISSION RECEIPT RESULT TARGET POLICY OUTPUT --apply
reel-workflow-native check-retention-capacity-plan PLAN SHA256 PACKET SOURCE ADMISSION RECEIPT RESULT TARGET
```

The first command exclusively creates a private canonical control file, using the
existing guarded receipt destination checks. Its `--apply` does not stage native
work. Rechecking first validates the caller's canonical plan digest. Both commands
open the original receipt/activation/WorkRef and complete native response metadata,
then capture the exact original request and actual Index checkpoint, candidate,
source/target selections and actor revisions under their ordered fences. Neither
reads output payloads, resumes publication, changes limits or acknowledges custody.

The policy schema is `reel.workflow-retained-owner-policy.v1`, with positive
`objects` and `logical_bytes`, capped by the adapter at 4,096 objects / 128 GiB.
These finite safety ceilings are not evidence of usable production capacity.
The proposal preserves the frozen per-job budget and reserves one extra object,
one changed path and 64 KiB for retaining its own immutable evidence. It refuses
insufficient policies, overflow, exceeding the original 128 changed-path bound,
unnecessary recovery and any existing/uncertain original retention work. Existing
work requires original-handle reconciliation rather than another proposal.

The observed pending control has 525 combined unique objects and 52 changed paths.
A 1,024-object / 1 GiB proposal requires at most 526 objects, 53 changed paths and
270,495,220 logical bytes including the conservative evidence reservation. This
proposal does not change the job's 512-object / 1 GiB budget or its original
264efd runtime. The original task, attempt, operation, retained target, source
repository, scratch, journals and other Waiting job retain their existing scope.

Five focused tests cover evidence reservations, independent object/byte/path
limits, overflow, strict schema, existing/uncertain work, unnecessary recovery,
exact pin authentication and changed state/request/plan rejection. The opt-in
native fixture additionally exercises the real pinned CLI over the actual pending
synthetic control, preserving the real Index, journal and original control bytes.
Its qualification evidence must be recorded separately after the frozen run.

Producer-applied rights/provenance review keeps the proposed policy distinct from
authority to release history, replace a runtime, select outputs or publish. The
comics-editor checklist requires complete corrected-issue assembly and contextual
review after the repaired part and assembly control. Neither checklist supplies
human creative approval or closes a finding.

The read-only qualification passed in 4.72 seconds at source
`2928ed19d444f51102ec2c0d040cc880c29f4b7c`. Evidence is in
`F:/src/tig-dev-temp/reel-retained-capacity-plan-20261120-v1`: 1,981 source files,
source-manifest SHA256
`af396705107df28897d61f6a106692e71ed4cccb624fc09bdc0269e62a01c09c`,
CLI `e278e6a4453289eff6cac4f74a8438ca2f5b3d077feb9f9d1d59aca6c364b907`
and fixture `833babe32847dae38bde90174a0ce0f9d2319982b2ccba4ab3064878defd158e`.
The actual exported plan is
`15af034bf386eb6e7b6b5c368b41144df62bd63e769622a7b365466370ed9005`.
Five actual CLI refusals pass, with the original journal, controls, actual Index
and other Waiting claims unchanged. Standard native tests pass (91 passed, 85
opt-in ignored), all 247 SDK pins and strict default/film Clippy pass. The
34-artifact certificate is
`27871ec96645dbca74b0ef98a5f7e14dbe9aa20eb8abae7f2af66a6e99c870f1`;
validation is `a60d77714e62231b50ce5d27b749339e4e74418cdd265cc772141eb214c18647`.

The subsequent `stage-retention-capacity` API checks the caller's pin, original
request and live source/target under their same fences. It reopens native output
and producer bytes before staging, retains the immutable policy record alongside
unchanged v1 registration evidence and preserves every existing target reference.
The original operation key is unchanged. An already-staged work is accepted only
with that exact policy record; no stale-plan regeneration or new work identity is
allowed. Its descriptor alone does not establish execution, retained ownership or
task completion. Effectful staging/process-exit recovery and original-runtime
completion require their own frozen qualification. Interrupted native execution
retains the existing original-WorkRef recovery protocol; broader crash coverage
remains open. Full corrected-issue assembly, contextual report and the wider
film/comic qualifications remain open.
