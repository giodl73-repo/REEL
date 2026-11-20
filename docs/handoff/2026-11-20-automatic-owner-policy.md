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
reconcile and complete exactly that original WorkRef. The frozen results below
qualify those specific boundaries; the first run's close refusal stays preserved.

The first frozen automatic run at source
`786203e09390e74997a045c72753be9a27b7e2d3` passed native import and lost-completion
reconciliation, then refused close preview with `budget/close-resource-bound`.
It observed 560 inherited objects plus a 48-object source: 610 combined objects,
611 including the policy record, with 52 changed paths. The old close adapter
still applied the job's 512-object limit to cumulative owner history. No close
ticket was exported and no close work began. The result stayed retained and its
source slot stayed protected until the original-operation follow-up below. The
interrupted-Prepare fixture did not start during that failed run.
Evidence, source and tools remain in
`F:/src/tig-dev-temp/reel-automatic-owner-policy-20261120-v1`.

The close adapter now validates the original owner-policy record and uses its
declared allowance for cumulative retained-owner handoff, including the unchanged
qualified SDK closer's repeated bound check. V1 close keeps its original budget.
The original source projection, task, attempt, activation and workflow runtime
stay unchanged. A policy-bound close must use its original retained actor/owner;
moving to another owner is a separate adoption boundary. The SDK closer may
canonicalize current reference paths while retaining every object and prior tree
root; original operation-specific policy links remain resolvable through their
immutable work selection.

The follow-up reopened that exact retained job and unchanged Index, then used a
separately pinned closing coordinator to stage/recover the same close WorkRef. Its
ticket retained the original lifecycle-runtime pin. Historical replay through that
original runtime after slot return and completed close replay left actual Index
unchanged. Do not rerun the original automatic fixture,
readmit its task, replace its runtime, reset the retained owner or remove history.
The new close coordinator is an explicit new close operation after the failed
read-only preview; it does not adopt a worker runtime or replace uncertain work.

The original-job follow-up passed in 24.46 seconds. The original job remains
`F:/src/tig-dev-temp/.tmp3dBPhV`, task
`2af3a178b728e044b869d7a6d0f1bbe175a1b81a58a34077c31e32b2797cefbb`,
attempt `owner-attempt-1`. Its lifecycle runtime remains
`d2ac0d431753c951ef8a6e68541cf0ade8383d8b626a4be02499546ed1c5f621`
(25,244,160 bytes), while the separate close coordinator is
`1217609b4ae771c0f4f2b013c72445ff35f555bb51d80bfb76083d5e0858cc60`
(25,261,568 bytes). No admission, publication, result or runtime was replaced.
All 560 historical entries remain; the source slot is idle under retained
coverage. Final actual Index checkpoint:
`a64a4cb06c067f0d35c6e008b955fc4c2b0184ebbde086eccb8c9c526e84ea88`.

The separate interrupted-Prepare case passed in 60.88 seconds, with job
`F:/src/tig-dev-temp/.tmprCD4zA`, task
`30eb63b18faab927f6458369aa002cf27c0d1056965aca3cdafafd9b09ec166f`,
attempt `owner-attempt-1`. It actually reached the SDK's native Prepared fault
boundary, preserved ImportPending and the staged WorkRef, then reconciled that
same work in the frozen CLI. Lost-completion replay, close, history preservation
and historical replay after slot return all pass. Its final actual checkpoint is
`cbffecd9c3a36c2a9d15da9885570b432ea173f2c08b2cbc56f6a9f8352b1c60`.

Both corrected qualifications use source
`648531cc6edd9cf77bb5fd00dc7438bf1b61c005`: 1,985 files verified against its
clean archive, source-manifest SHA256
`041be51831f327e141ad2afd4b97394210ef8740da51d37a3f8b68063a085fc3`.
The fixture executable is
`a8945b092bf3bd54a3ed1f3368e3507fc97522a4cf6548442284615ea0909dad`
(30,824,960 bytes). Standard native tests pass (96 passed, 89 opt-in ignored),
all 247 SDK source pins and strict default/film Clippy pass. The original source
packet/tool hashes remain in the failed-run directory; no source qualification
is inferred from an ancestry commit alone.

Combined evidence is in
`F:/src/tig-dev-temp/reel-automatic-owner-close-20261120-v1`; the separate Prepare
case is in `F:/src/tig-dev-temp/reel-automatic-owner-prepare-20261120-v1` and pins
the same exact source packet. The 87-artifact `qualified-report.json` hash is
`f2a0e181ee71dbb5abd94e2411a27b67ed08c3c4e983043bb053704153131d9d`;
`qualified-validation.json` is
`9da91240337cc03c2959045d76d95849ef27b233a7536017f13be6133fcdd809`.
Earlier retained-control/planning certificates still verify. All private source,
scratch, failed controls, published results and metadata roots remain preserved.

Producer-applied rights/provenance review keeps the policy in private controls and
separates technical custody from selection, consent and release. The comics-editor
checklist still requires complete corrected-issue assembly and contextual review.
No actual S5/BBJ media or active lane is adopted. This does not lift the per-job
512-object projection limit, qualify 600-resource / 7.94 GB episode production,
resolve the separate 3.6 GB native I/O stop, or establish production profile
adoption/rollback. Those remain explicit completion requirements.
