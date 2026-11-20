# Independent reader ceilings at the native parent boundary

The actual full BBJ reading intake reached Retained and closed correctly. Its
first assembly advancement then returned `Conflict/composite-child-store-actor`
before any child admission. The two profiles use identical enrolled pool/store
identities but different per-object read ceilings: 128 MiB for the original
parent and 256 MiB for the new consumer. Their declared task/snapshot/return
budgets are independent; a read ceiling is not another storage identity.

The adapter now compares the full enrolled pool and exact store digest, while
keeping caller ceilings separate. It still rejects changed tool paths, pool
configuration, store identity, metadata/coordination roots, Index identity or
reuse of the retained target as the child actor. Before admission, each actual
child input must fit that child's own reader ceiling. Parent byte verification
continues under its original pinned profile. Child reads continue under their
own profile and the unchanged qualified SDK's byte/namespace/object gates.
No SDK files or admission limits are changed.

The tests cover distinct ceilings in both directions, wrong store, wrong pool,
changed annex executable and insufficient/zero child read capacity. Real native
preparation and acceptance of the original BBJ step remain the qualification
gate after the standard checks pass.
The standard native suite now passes 105 tests, including the two new boundary
tests, and verifies all 247 unchanged SDK pins. Strict Clippy passes. The
preparation command reports the raw frozen-file SHA256 expected by
`accept-expanded`; task/source canonical identities remain separate.

## Preserve the original operation

Do not shrink the consumer's ceiling, mutate its immutable runner config or
replace its worker runtime merely to pass the earlier comparison. The old
runner stopped before exporting `pending-001.json` or admitting a child.
It can recover an original admission accepted by a separately qualified SDK
coordinator, through its existing `Prepared::recover` path.

The new command performs only planning/freezing:

```text
reel-workflow-native prepare-recipe-step RECIPE BASE_PLAN RUN_BINDINGS CONFIG TASK NEW_FROZEN --apply
```

It uses Rust recipe scheduling and exact retained-parent verification to derive
the next template; no shell-generated dependency digests or task control maps
are needed. It holds the named original runtime without invoking or impersonating
it. Effectful adoption, dispatch, worker and lifecycle current-executable checks
remain unchanged.

For the actual BBJ case, use the original recipe/base bindings/config, task
`bbj-cp02-reading-issue-v1-compose`, attempt `reading-compose-attempt-1` and
operation of the same name. Export the exact frozen step to the original owned
runner's unused `native-recipe/pending-001.json`. Pin the fixed coordinator's
bytes/source. Then use its existing `accept-expanded` interface against that
frozen pin, saving the original admission receipt outside the capsule. On
uncertainty, recover that exact admission; never substitute a fresh operation.
Resume the original runner/runtime with the unchanged configuration and a new
observation log. It must recover the accepted WorkRef and construct its original
handoff before worker execution.

Preview v5 remains a pinned baseline package and does not contain this subsequent
fix. Existing jobs keep their original runtime; new qualification uses a separate
pinned admission coordinator rather than silently retargeting them.

## Actual continuation

The fixed coordinator is source `3ae161142259d1b4180af3d8d57519ed4c3884aa`,
SHA256 `2537f822cfc2a94281ddf1e6d26556f586151439cbae28c41d3ba139a97857e0`.
Its exact file pin is in the BBJ root's
`reader-bound-admission-fix/coordinator-tool.json`. `baseline.json` records the
original refusal and hashes of the unchanged recipe, config, bindings, runner
intent and state-000 before any pending step existed.

One `prepare-recipe-step` process was started at 11:22:10 local time on November
20 (observed PID 3880; tool session 81206). It writes
`reader-bound-admission-fix/prepare-result.json` and, only after successful
preparation, `native-recipe/pending-001.json`. It was still live at the last
observation. Revalidate that original process/result before any repeat call.
No child acceptance has been requested yet. Do not mistake preparation for
admission or a frozen control for an available idle actor.

On successful preparation, compare the reported raw frozen SHA256 to the file
bytes, then accept that exact original admission through this coordinator's
`accept-expanded` command, with a new receipt in the fix directory. Preserve its
original work and use `recover-expanded` on uncertainty. After acceptance,
resume the original `a62c8257...` runner against its original directory/config
and a separate new output log; do not rerun the producer under a fresh identity.
