# Immutable retained-owner capacity recovery plans

The [actual capacity diagnosis](2026-11-20-retained-owner-capacity.md) identifies a
cumulative-owner limit, not a failed render or oversized job input. The native
adapter now exports and rechecks an immutable proposal for a separate, finite
owner-history allowance. This is a read-only planning boundary; there is no
capacity-policy application API yet.

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

Remaining recovery work: implement a checked application transaction that retains
the immutable policy evidence, preserves the original retention operation and
validates payloads, plus interrupted-stage/execute reconciliation and proof that
the frozen original runtime can acknowledge/resume that exact work. Recheck and
mutation must share the same fences and expected revisions; a successful read-only
check must never be treated as an admission token. Full corrected-issue assembly,
contextual report and the wider film/comic qualifications remain open.
