# Explicit snapshot and managed projection budgets

The S5 consumer audit exposed two additional legacy byte boundaries: the native
v1 snapshot reader caps summed task inputs at 64 MiB, and the managed projection,
retention and close adapters cap logical resources at 1 GiB. The separate comic
64 MiB content contract does not establish a film limit. Neither legacy behavior
is silently raised.

## Implemented contract

`SnapshotProfile` accepts `reel.workflow-native-snapshot.v2` and
`reel.workflow-native-composite-snapshot.v2` only with an explicit `budget`:

```json
{
  "schema": "reel.workflow-snapshot-budget.v1",
  "input_bytes": 8589934592,
  "input_object_bytes": 4294967296,
  "projection_bytes": 34359738368
}
```

This is an illustrative envelope, not an adopted S5 profile. Input bytes are
summed from every consumed task input, including aliases. Each consumed input
and each declared annex payload must fit the object allowance, even if a tiny
preview is selected instead. Object allowance is positive and at most input
allowance; input allowance is at most 64 GiB. Projection allowance is at least
input allowance and at most 128 GiB.

The projection accounts for distinct protected resources, metadata, declared
streams, runtime pins and, during response/retention/closing, returned objects
and prior retained references. Metadata and history need explicit headroom.
Overflow or excess refuses; no split, omitted closure or fresh actor is inferred.

Exact budget bytes enter the portable native bindings v2 declaration and thus
the task key, source binding and original admission. Budget changes cannot reopen
an old receipt. Expansion and recipe runners carry the version through frozen
composite snapshots. V1 profiles omit the optional field and preserve declaration
bytes and their 64 MiB/1 GiB limits.

Only byte budgets change. Task inputs remain 128; scope paths 256; metadata
512 documents/16 MiB; projection objects 512; actors 16 total; changed paths 128
per phase. The existing qualified single-store SDK and source pins are unchanged.
Composite upstream contracts independently retain their 64 MiB limit. Film v2
dispatch remains gated; generic candidate-return and resolution budgets remain
unchanged. This does not admit an E7 closure of 600 resources.

## Evidence and review scope

Three ordinary synthetic tests cover a declared 3,600,000,001-byte exact Git
record/annex pointer, unchanged legacy declaration bytes, schema/budget pairing,
object refusal, refusal of a large unconsumed selected payload, full-projection
headroom and stale-receipt refusal after a budget change. The admission fixture
uses synthetic metadata/index state. It does not read, publish or claim
availability of a multi-GB native payload. Actual native large-input staging,
retention, close/reuse history and interrupted recovery remain qualification
gates.

All 78 standard native tests pass with film support, including verification of
all 247 SDK source pins and the two unchanged qualified lifecycle files. Strict
all-target Clippy passes both with film and with no default features;
`git diff --check` passes. The 61 explicitly ignored native/large-effect fixtures
were not rerun. The test log is retained outside Git at
`F:/src/tig-dev-temp/reel-snapshot-budget-tests-20261119.log`.
Its SHA256 is
`53ccb3accc0e33b73ee1c0a51ab62b89e3fb797635fe94340ce1e507a5bcb148`.
Implementation source is `c2d83cfd885c690693df07a021550fbbc156c3bf`.

The `.roles/ROLE.md` rights/provenance checklist was applied to this technical
contract: larger allowances do not grant source use, egress, creative approval,
selection, delivery or release. Editing and complete bilingual conformance are
still assessed at the film/edition layer; no pacing or approval subject is
changed here. This is a producer checklist application, not independent review
or human approval. No real S4/S5 media, job, profile, recording or approval changes.
