# Resume a trusted native job from retained checkpoints

`reel-assets --root <project> resume-job <original-job.json> <checkpoint.json>`
creates a fresh admitted workspace from an unchanged, registered original job.
It copies registered checkpoint files as private working data, then executes the
original steps beginning at the explicitly selected step. The original job,
tool pins, inputs and output bounds are unchanged. V1 preserves the workspace
budget and all step timeouts; newer explicitly registered windows are described below.
The original workspace is not accessed or removed by this operation.

The checkpoint descriptor uses `reel.native-job-resume.v1` with these fields:

- `job_sha256`: exact SHA-256 of the original job JSON bytes.
- `source_job_id`: provenance identity of the interrupted job.
- `start_step`: zero-based index of the first original step to execute.
- `files`: relative workspace paths mapped to exact `sha256` and `bytes`.

The original job, descriptor and checkpoint objects must already be registered
and present in active caches. Their bytes are freshly verified and pinned for
the operation. No archive lookup is performed. The checkpoint files must fit
inside the original scratch allowance and cannot overlap original inputs, tools,
inline files, outputs or runner controls. Ordinary workspace monitoring and final
output retention remain active; normal success retires only the new workspace.

This is a trusted production operation, like `run-job`. It does not assert that
skipped steps succeeded or that arbitrary checkpoint data is build-ready. The
native continuation must validate its checkpoint's source scope, content, clocks
and tool identities before using it, and must retain its ordinary final checks.
The result and retained retirement evidence record the descriptor hash, original
job identity and first executed step separately from build or human approval.

An explicitly registered `reel.native-job-resume.v2` descriptor may additionally
declare `step_timeout_seconds`, an object mapping one original step index to a
longer execution window, capped at 7,200 seconds. The index must be at or after
`start_step` and identify an existing step. Empty, shorter, equal, out-of-range,
multiple-step and over-cap windows are rejected; V1 cannot carry an extension.
This provides bounded recovery when a complete native verification needs more
time. It changes no original JSON bytes or native validation, memory/disk bounds,
tool identity, command or output requirement. The result and failure diagnostics
explicitly report the effective extension rather than claiming the original
timeout was enforced. Ordinary `run-job` requests retain their one-hour default
step cap. Fresh requests may explicitly declare `step_timeout_limit_seconds`
(at most 86,400); each step must fit that finite declared limit. This does not
change the separately scoped, hash-bound recovery-window rules below.

For a fresh, unchanged recipe, `run-job-window <original-job.json>
<registered-execution-window.json>` executes every original step. Its separately
registered descriptor contains only `schema: reel.native-job-execution-window.v1`,
`job_sha256` and the same one-entry `step_timeout_seconds` map, capped at 7,200
seconds. It provides no checkpoints and skips no steps. Both original recipe and
descriptor must be registered and freshly verified in active caches. Results
and failure diagnostics use a separate `execution_window` evidence field. This
does not mutate stored recipes, default `run-job`, tools or build checks.

## Explicit temporary workspace windows

`reel.native-job-resume.v3` and `reel.native-job-execution-window.v2` may declare
`workspace_budget_bytes`, strictly larger than the original recipe's budget and
at most 16 GiB. This is an explicit per-job operational exception, never a
default or project-policy update. The descriptor must be registered and bound to
the exact original job bytes. Obtain the producing project's actual resource
authorization before invoking it; accepting a descriptor grants no human approval.

The runner reserves and monitors the effective workspace allowance. All existing
free-space reserves and concurrent reservations remain enforced. Additional
workspace headroom is also added to the checkpoint scratch intake allowance.
Canonical input resolution, retained-object size limits, output bounds and
publication still use the unchanged project policy. The original `job.json`,
commands, tool pins and final validation steps are unchanged. Results and failure
evidence identify the effective allowance; successful results also preserve
`original_budget_bytes`. A window does not silently extend any step timeout.

V3 recovery may additionally seed `metadata_outputs`, keyed by exact original
output role, for already-produced or freshly checked prerequisite metadata.
Each must be a registered JSON object, target a declared `.json` output with a
maximum bound of 1 MiB, and fit that original bound. Media outputs cannot be
seeded. Ordinary checkpoint files still cannot overlap inputs, outputs or runner
controls. Metadata is copied privately and its restored role is explicitly
reported; it is retained evidence, not a newly executed preflight or approval.
The producer must freshly establish any time-sensitive dependency facts and the
remaining native steps must validate their checkpoints and final products.

Legacy descriptors reject these fields. A fresh V2 window skips no steps and
accepts no checkpoint or metadata-output seeds. V3 recovery and fresh V2 windows
can independently request the existing single bounded timeout window; at least
one explicit extension is required. Neither command starts a daemon, restores an
old workspace in place, accesses archives, or changes sharing/publication rights.
