# Versioned retained dependency streams

Episode composition needs to consume retained opening, ending and component
masters. Legacy upstream-inputs v1/v2 bind exact parent results but cap the sum
at 64 MiB. Buffered borrowed reads are also unsuitable for multi-GB media.

## Contract and native port

`reel.workflow-upstream-inputs.v3` requires a `streaming` object:

```json
{
  "schema": "reel.workflow-upstream-streaming.v1",
  "input_bytes": 8589934592,
  "input_object_bytes": 4294967296
}
```

Object allowance is positive, at most total allowance and at most 16 GiB.
Total allowance is at most 64 GiB. Every selected parent artifact fits both
the summed logical-input budget and per-object allowance. Exact allowances
enter the upstream control pin and child task identity. Aliases do not reduce
logical totals. V1/v2 reject a streaming extension; omitted fields preserve
their exact serialized bytes and 64 MiB behavior. V3 retains v2 cross-snapshot
semantics: each original parent packet/result/attempt/project/dependency digest
and output or worker-receipt selector remains exact. Matching payload bytes
alone cannot substitute for that lineage.

Native frozen origins archive the complete original bindings and parent
evidence. `BorrowedInputs::with_stream` opens an exact guarded native dependency
while retaining original parent proof, runtime pins and retained-owner fences.
It does not capture the complete file in a `Vec`. The callback enforces its own
bounded consumption/materialization. A callback error still preserves the native
gate for that session. Reopening an already streamed object is refused before
another provider call; the consumer must reuse its verified local copy for
aliases. Buffered controls may be reused through a cursor. V3 buffered reads
over 64 MiB refuse before allocation or native access.

This is borrowed parent custody, not durable child admission. Composite child
admission still verifies live original parents and acquires its own declared
claims before waiting/export. Snapshot input/object/projection budgets, reader
profiles, parent retained-result checks and all existing object/actor/path
limits remain independent. No source slot is implicitly released and no claim
expires. Native SDK transfer/reopen deadlines remain unchanged.

The original qualification below used explicitly authored v3 bindings.
The subsequent [streaming expansion port](2026-11-19-streaming-expansion.md)
now carries per-step settings through v2 templates/recipes and emits exact v3
bindings. Automatic episode partitioning/composition and complete film dispatch
remain open.

## Tests and qualification boundary

A synthetic seven-parent metadata fixture totals 7,936,104,893 declared bytes,
including 3.36 GB and 2.9 GB objects. The sizes are synthetic and do not describe
an inspected E7 partition. It checks legacy refusal, v3 acceptance, total/object
ceilings, changed-control/key refusal and byte-identical legacy restoration.
Schema/envelope tests cover the numeric maxima. Native component tests cover
large buffered-read refusal, exact-artifact checks, alias refusal and reuse of
bounded captured controls without provider access.

An opt-in read-only fixture targets the existing retained synthetic comic. It
checks complete original parent proof, frozen v3 composite metadata, streamed
exact bytes, caller allowance refusal, repeat-open refusal and equality of the
actual native Index checkpoint. It grants no child admission, native publication,
new capacity, task completion, selection or adoption.

The fixture passed in 29.11 seconds (29.88 seconds including process launch),
streaming a 10,138-byte retained synthetic PNG after complete original parent
verification. Frozen v3 composite metadata resolved, a too-small caller allowance
refused, the second open refused before provider access and the actual native
Index checkpoint remained byte-equal at sequence 19. This is a small native
dependency fixture, distinct from the separate synthetic 7.94 GB metadata test.

Source `ad2d52a6e69bdc7a12a07534590d7cc2608f3375` is archived with all 1,942
tracked files individually hash/length compared against the working tree.
Frozen Windows test executable: SHA256
`d9043bdd242a18bf49c1f2f0ea83a90a4ba38e0ad50718bf5e46cc9372b760af`,
43,889,664 bytes. All 115 workflow, 84 standard native and 31 HTTP tests pass;
all 247 SDK source pins and strict all-target Clippy for workflow and native
with/without film pass. The 63 ignored native fixtures were excluded from the
standard suite; only this named existing-parent fixture ran separately.

Private certificate:
`F:/src/tig-dev-temp/reel-streamed-upstream-20261119-v1/qualified-tools.json`,
SHA256 `d604a531a0e846cc6a68cf5d492b6aa043161f9ddf618d91eeb54db69ab3d674`.
It binds source archive/manifest, frozen executable, original start/outcome,
native result/checkpoints and all three standard test logs. Session 15241
terminated successfully; the parent and original native jobs remain intact.

This does not qualify multi-GB native dependency bytes, the full 600-resource
closure, component scheduling, complete bilingual assembly, retained intermediate
handoffs at scale or owned consumer profile adoption/rollback. The producer's
editor and rights/provenance checklist preserves order, original source identity
and exact approvals; numeric/byte validation does not approve an edit, translation,
voice, likeness or release. No real S4/S5 media or jobs are changed.
