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
