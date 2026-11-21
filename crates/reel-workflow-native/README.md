# Qualified workflow SDK intake (W3a.1)

This isolated workspace compiles the exact lifecycle SDK and git-asset capacity
store dependencies needed by the [film-first workflow plan](../../docs/workflow-pr-plan.md).
It has its own lockfile; accepted producers continue using `vendor/tig` and the
root workspace lockfile. Neither is upgraded by this intake. No comic renderer,
HTTP provider or workflow producer is linked.

This slice supplies source pins and verification, not a native workflow runtime.
It has no binary, admission, rendering, publication, capacity mutation, retention,
job cancellation, recovery or profile adoption command. Those consumer adapters
belong in subsequent W3a changes with explicit lifecycle qualification.

| Source | Exact scope |
| --- | --- |
| `vendor/tig-workflow` | 247 unchanged qualified files. Working-tree source manifest `f89d836d82b1a6240fbe77f4fd53b488ed57ceb303111642be53fd668ffe2cfe`; the recorded ancestry commit alone is insufficient. |
| `vendor/reel-workflow-lifecycle` | Original slots module and its original tests from EVID-151, plus license. Combined qualification digest `1c3d0b7db734bc946c3fd482134cabaac1c023f67ea7d2f19a0bf4c3f63d88e3`. This intake preserves these two files; it does not compile or qualify their future adapter/fence shim. |
| `vendor/git-asset-capacity` | 379 unchanged files from source commit `fa6697cf62eb61bf9477fd62b6493a024ead4e49`, manifest `25e4a2173488842fe61201a48149ac4eb5f91f736f832a1a82e9e3e0401fddb4`. Upstream license is included separately because the original manifest omitted it. |

The capacity implementation was merged in [git-asset PR #67](https://github.com/giodl73-repo/git-asset/pull/67),
which closed [issue #64](https://github.com/giodl73-repo/git-asset/issues/64).
Every imported capacity manifest entry was checked against that exact clean
source checkout. This is not an upgrade to arbitrary latest git-asset source or
an installed lifecycle-capable distribution. Older lifecycle types and newer
capacity types remain in distinct namespaces; compiling both does not make them
interchangeable. Consumer lineage/rebind and original-operation recovery still
need their explicit adapter boundary.

Validation from the repository root:

```text
cargo fmt --manifest-path crates/reel-workflow-native/Cargo.toml -- --check
cargo clippy --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked
cargo test --manifest-path vendor/git-asset-capacity/Cargo.toml -p git-asset-store --test metadata_capacity --locked
```

Three source tests pin the original manifests, every imported file's length/hash,
licenses and the complete file set. Unlisted sources and line-ending changes
fail. `.gitattributes` preserves these qualified bytes across platforms. CI runs
the isolated checks on Windows and Linux; root tests retain their previous scope.
The original upstream capacity suite additionally exercises synthetic planning,
history preservation, writer fencing, crash reconciliation, stale plans and
rollback refusal: 10 tests pass locally on Windows. Its separate near-production
test requires an explicitly pinned legacy executable and remains ignored; no
claim is made for that consumer qualification here.
The vendored source tests qualify source intake, not actual episode-scale
admission, native byte availability, production custody, store adoption or human
approval. No existing cache, store, runtime profile, asset, approval or active job
is modified by building/testing this intake.
