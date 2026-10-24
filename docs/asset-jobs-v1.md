# Bounded job inputs and scratch lifetime

`reel-assets --root <project> run-job <manifest.json>` executes an explicitly
trusted recipe in the project's configured active caches. It does not start a
daemon or access archive tiers. Existing staging/result names remain compatible.

Production version publication has a separate [asset-management completion hook](asset-version-diff-v1.md).
After registering/selecting the exact accepted version, call `version-manage` and retain
its `managed: true` receipt before completing the production job or PR. This is an explicit
producer integration; `run-job` success and scratch retirement alone do not provide it.

## Inputs and executables

Inputs and tools must be registered exact SHA-256/byte pairs. Immutable inputs
and executable tools use same-volume hard links where possible. Open Windows
handles deny writes through either link for the job's lifetime while allowing
other readers and removal of temporary links. Cross-volume views use verified
private copies with the same read-only handle protection.

An input that the recipe must modify is named explicitly in `writable_inputs`:

```json
{"writable_inputs": ["inputs/editable.bin"]}
```

That input receives a private copy. Original cache data remains protected.
`linked_input_bytes` and `copied_input_bytes` in the result expose materialization
cost, including declared tool files. Admission remains conservative: the budget
still covers input sizes, scratch and atomic output publication, even when a
link avoids their physical allocation.

Runtimes needing adjacent DLLs may be declared as `installed_tools`, mapping an
alias to an absolute installation path and registered SHA-256/byte pair. The
installation is verified and pinned read-only; each step rechecks its hash.
Installed executable bytes are not copied or counted as staging input bytes.
This contract is for trusted programs, not a security sandbox.

## Default completion and failure behavior

Normal jobs publish declared outputs, register the exact request and retained
results as private candidates, preserve bounded compressed diagnostics, verify
canonical active-cache copies, and retire the exact job workspace automatically.
The returned result sets `workspace_retained` to false and includes a
`retirement_evidence` SHA-256/size/cache URI descriptor. Existing output
descriptors retain their verified canonical `path` fields.

Failure also retires scratch after salvaging any present, regular declared
output within its declared size bound. Salvaged outputs remain unapproved
candidates. The error includes the retained diagnostic cache URI. Evidence
contains the original request, pre-retirement result, declared retained output
descriptors, and up to 16 KiB from each bounded step log/PID file. The snapshot
records the workspace as it existed before removal; it is not a claim that the
workspace still exists afterward. Registration grants no selection or approval.

Unsafe output files, failed registration, or unverifiable retained copies hold
the workspace and return an explicit retirement failure. The finalizer rejects
reparse points and validates the canonical target against the exact admitted
cache staging parent before recursive removal. Canonical CAS objects, shared
LFS stores, and other worktrees are never removed by job retirement.

Forced termination of the runner kills its process tree but can leave an
interrupted workspace. Such work requires a separate inventory and guarded
retirement; a result-free folder is not permission to delete its contents.

## Migrating recipes and diagnostic exceptions

Every post-run artifact must be a declared output. Read its returned canonical
output path, or retain a declared package and read its members. Do not read
`result.workspace/…` after a normal job, and do not re-ingest published outputs
from staging. Retain newly required stems, receipts and alignment data explicitly
before adopting this runtime. Existing pinned producer runtimes are not changed
by updating this crate in another worktree.

For an explicit bounded debugging session, `retain_workspace: true` preserves
the prior inspection behavior. It is not a production recipe default. Such a
workspace must later be retired with the guarded `discard-job-scratch` command
after its manifest and outputs are registered and its reservation is gone.
The exception must not become an automatic retry/retention policy.

The job budget is a watchdog/admission contract, not an OS filesystem quota.

After media execution, completed-output promotion receipts and retirement
registration wait up to 30 seconds for the producer metadata lock. They never
rerun a tool, repeat a media copy while waiting, or release another writer's
lock. Ordinary metadata and capacity transactions retain their two-second
wait bound. Publication still fails closed if the bounded wait expires.
Programs remain responsible for placing intermediates inside staging. Per-job
cleanup avoids accumulation but does not replace asset version/tiering policy.

## Validation

Windows synthetic jobs cover immutable write rejection, explicit writable-copy
isolation, installed runtime pinning, concurrent read jobs, success/failure
retirement, retained candidates/logs, timeout process-tree termination, explicit
debug retention, and safe holds for oversized output or registration failure.
These tests use no production project assets or archive drive.

```text
cargo test -p reel-assets --all-targets --locked
cargo clippy -p reel-assets --all-targets --locked -- -D warnings
```
