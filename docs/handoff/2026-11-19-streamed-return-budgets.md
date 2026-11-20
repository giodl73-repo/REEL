# Task-bound streamed return budgets

Season 5 needs multi-GB returned masters, independent of the comic contract's
64 MiB input closure. REEL's legacy response and retained-result checks capped
the full return at 128 MiB even when explicit film intent allowed more.

## Contract and implementation

An optional `returns` object inside the pinned v2 snapshot `budget` declares:

```json
{
  "schema": "reel.workflow-return-budget.v1",
  "total_bytes": 10737418240,
  "object_bytes": 8589934592,
  "verify_millis": 300000
}
```

This example is not an adopted project profile. Total allowance is at most
16 GiB and no greater than the snapshot's full projection allowance. Object
allowance is positive and no greater than total allowance. Verification deadline
is positive and at most 300 seconds per file, checked between blocking reads.
It does not interrupt an OS read in progress or override a native SDK timeout.

Every output contract's maximum must fit the object allowance. All declared
maxima, including optional outputs, must fit the total allowance; tool tasks
also reserve the existing 1 MiB worker-receipt ceiling. These checks run with
snapshot binding/admission, before any returned-file read or candidate effect.
Actual returned artifacts, aliases and worker receipts are also summed with
checked arithmetic. Content deduplication does not reduce that logical budget.

The exact allowance is part of the portable native declaration, task key and
original admitted source binding. A returned `reel.workflow-returns.v1` manifest
still supplies only owned paths. It cannot change allowances or operation
identity. Changing a budget requires a new task identity; an old admission
cannot be rebound. Omitting `returns` preserves existing declaration bytes and
the 128 MiB legacy return allowance, including for existing v2 snapshot budgets.

Ordinary returned files are hashed using a fixed 64 KiB buffer with exact
length/EOF/SHA checks. Interrupted reads retry; corruption, truncation, trailing
bytes and deadline exhaustion refuse. Original held files and guarded directories
remain live across candidate registration and native publication/reopen. The
native publisher receives the explicit object allowance. Retained-result
preflight and historical verification use the same task-bound return budget.
Image decoding, structured-review controls, deterministic producer verification,
native reader profiles and full protected projection retain their separate
limits. In particular this does not activate gated v2 film dispatch.

## Qualification scope

Regular tests cover bounded stream verification, interrupted reads, corruption,
truncation, trailing bytes, I/O classification, deadline checks after a blocking
EOF, logical alias/receipt accounting, overflow, declared object/total ceilings,
legacy declaration bytes, idempotent small candidate registration and refusal of
changed budgets under the original admission. The metadata fixture does not
establish large native payload availability.

An explicit read-only fixture uses the existing reserved 3,600,000,001-byte
synthetic source, verifies the original known hash through this return adapter,
refuses the legacy allowance and holds a mutation guard. It allocates no new
capacity, creates no native job, publishes no payload and completes no task.
The fixture passed in 25.66 seconds (26.76 seconds including process launch).
It verified the original hash, refused the legacy allowance before reading and
refused an attempted write while the file guard remained live. The existing
source/reservation is preserved. No new native admission, publication, capacity
reservation, task completion or consumer adoption occurred.

Source: `750eedd9ef11e2b7c9136bf31ba1294acb1348a7`; all 1,940 tracked files
were archived and individually hash/length compared against the working tree.
Frozen release test tool: SHA256
`2d24b367ef9fa6494f46a8ac0ef8638185d7fc23428545f659abd474f31d0f78`,
15,434,752 bytes. All 82 standard native tests and the 247 SDK source pins pass;
strict all-target Clippy passes with film and with no default features, and
`git diff --check` passes. The final snapshot suite completed in 136.86 seconds.
The source's 62 explicitly ignored effect/large fixtures were excluded from the
standard suite; only the named read-only large-return fixture ran separately.

Private certificate:
`F:/src/tig-dev-temp/reel-streamed-return-20261119-v1/qualified-tools.json`,
SHA256 `f81ee416ebea33a74bf1cf0bcc417bd9349a5ac67f95702c76cf3f443c94cd96`.
It pins the source archive/manifest, frozen executable, exact input, original
start/outcome records, large-return log, final standard-test log and both Clippy
logs. Qualification session 44485 and regression session 9559 both terminated
successfully; neither is waiting or needs a replacement run.

## Tig boundary and remaining work

The unchanged qualified SDK's
`crates/tig-git/src/annex/workflow/publication.rs::transfer` and
`crates/tig-git/src/annex/workflow.rs::hold_copy_with_guards` each enforce a
fixed 30-second transfer/hash deadline. Publication can hash the source, copy it
into an owned native staging attempt and hash again on reopening. Reader and
publication profiles permit up to 64 GiB, but byte allowance alone does not
establish those timings or interrupted multi-GB recovery. Generic I/O errors
remain conservatively mapped; REEL must not parse stderr or substitute bytes.

Large native admission/staging, publication, retained acknowledgement,
close/reuse history and interrupted reconciliation require explicit qualification
on the pinned implementation. If this SDK cannot meet the declared fixture,
Tig needs a caller-bounded transfer/reopen deadline, exact source/tool pins and
typed uncertain/recovery outcomes while preserving original work, source,
native staging and retained-owner handles. Do not edit the vendored SDK to
silently expand its qualified behavior, create a new cache or allocate a fresh
operation after an uncertain publication.

Full 600-resource composition, film worker/return verification at scale,
bilingual assemblies, complete delivery and owned adoption/rollback remain open.
The producer rights/provenance checklist keeps technical verification separate
from voice/likeness consent, creative approval, selection and release. No actual
S4/S5 media, recording, job, profile or approval changes.
