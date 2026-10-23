# Resume a trusted native job from retained checkpoints

`reel-assets --root <project> resume-job <original-job.json> <checkpoint.json>`
creates a fresh admitted workspace from an unchanged, registered original job.
It copies registered checkpoint files as private working data, then executes the
original steps beginning at the explicitly selected step. The original job,
tool pins, inputs, output bounds and workspace budget are unchanged. V1 preserves
all step timeouts.
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
timeout was enforced. Ordinary `run-job` retains its one-hour step cap.

For a fresh, unchanged recipe, `run-job-window <original-job.json>
<registered-execution-window.json>` executes every original step. Its separately
registered descriptor contains only `schema: reel.native-job-execution-window.v1`,
`job_sha256` and the same one-entry `step_timeout_seconds` map, capped at 7,200
seconds. It provides no checkpoints and skips no steps. Both original recipe and
descriptor must be registered and freshly verified in active caches. Results
and failure diagnostics use a separate `execution_window` evidence field. This
does not mutate stored recipes, default `run-job`, tools or build checks.
