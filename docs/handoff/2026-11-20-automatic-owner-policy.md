# Automatic finite owner-history policy for new jobs

The [original-operation recovery](2026-11-20-retained-capacity-recovery.md) completed
an old frozen control without replacing its runtime. New jobs now have an explicit
versioned policy path so accumulated history need not cause repeated manual
capacity recovery. This policy is separate from the frozen job input/projection
budget and uses the existing qualified Rust lifecycle and native Index.

`reel.workflow-native-handoff-binding.v2` requires `retained_owner_policy`:

```json
{
  "schema": "reel.workflow-retained-owner-policy.v1",
  "objects": 1024,
  "logical_bytes": 1073741824
}
```

The binding still includes the exact original admission receipt and retained
actor/owner target. Its full canonical artifact is pinned by the journal ticket
before execution. The v1 binding requires this field absent and serializes exactly
as before. A v1 field injection, v2 missing policy, invalid/unbounded allowance or
changed ticket binding is refused. The wrapper profile remains
`reel.workflow-native-handoff.v1`; the nested binding is independently versioned.
Existing frozen runtimes/profiles are not rewritten or adopted implicitly.

Recipe adoption uses `reel.workflow-advance-config.v2` with the same required
policy. V1 keeps the field absent. The existing immutable config/intent binds its
exact bytes, so changing the policy cannot reinterpret an already adopted job.
Before accepting the job, adoption checks a known-input/current-owner-history
lower bound. The waiting handoff performs the same check before provider work.
It is not an output forecast, capacity reservation or guarantee against later
owner growth; the actual complete response is checked again at retention.

After response registration, the new lifecycle automatically stages a
`reel.workflow-retained-owner-plan.v1` under original source/target fences. This
mode permits ordinary policy admission even if the legacy job projection limit
also fits. It unconditionally enforces the declared owner allowance and reserves
one object, two changed paths and 64 KiB for its policy evidence. The policy itself
is bounded at 4,096 objects / 128 GiB. The SDK 128 changed-path / 16 actor limits
remain independent; no engine, cache, actor enrollment or lease service is added.

The native work keeps unchanged v1 registration evidence and operation identity,
adding both canonical policy-object coverage and an operation-specific
`retained-policies/` link. An interrupted caller reopens that exact named original
record instead of scanning all history or generating a plan against newer state.
Its request, policy, canonical evidence and actual work-object/byte counts must
match. An interrupted native Prepare first validates that exact policy evidence,
then reconciles only its original WorkRef before opening a settled claims guard.
An unrelated pending operation is not a reason to start new work.
Missing/conflicting evidence stops the job with claims and scratch intact.
Execution and retained-owner acknowledgement then use the normal original-work
protocol. A bounded owner policy never authorizes discarding old versions or
silently increasing an allowance.

Completed and historical handoff verification rechecks the same immutable owner
policy while holding retained coverage. It does not require the source actor to
stay active. Generic retained-byte APIs retain their custody scope; the handoff
binding supplies this additional workflow policy requirement.

Tests cover legacy serialization/hash preservation, version/field consistency,
ticket policy replacement refusal, known-history pre-execution refusal, independent
policy enforcement when job projection fits, evidence path reservations and exact
adoption-config intent. A dedicated opt-in native fixture declares an existing
synthetic payload, creates only its own private metadata Index, seeds 560 history
objects in five 112-path phases, then exercises automatic native retention with a
1,024-object owner policy. It must preserve every old reference, reject policy
replacement, recover lost completion without new native work, close into retained
coverage and verify historical replay after source-slot return. A second case
records import intent, registers the response, stages the same owner policy,
injects a failure at native Prepare, then asks a separate frozen CLI process to
reconcile and complete exactly that original WorkRef. Both frozen-source
qualifications are pending.

Producer-applied rights/provenance review keeps the policy in private controls and
separates technical custody from selection, consent and release. The comics-editor
checklist still requires complete corrected-issue assembly and contextual review.
No actual S5/BBJ media or active lane is adopted. This does not lift the per-job
512-object projection limit, qualify 600-resource / 7.94 GB episode production,
resolve the separate 3.6 GB native I/O stop, or establish production profile
adoption/rollback. Those remain explicit completion requirements.
