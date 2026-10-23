# Resume a trusted native job from retained checkpoints

`reel-assets --root <project> resume-job <original-job.json> <checkpoint.json>`
creates a fresh admitted workspace from an unchanged, registered original job.
It copies registered checkpoint files as private working data, then executes the
original steps beginning at the explicitly selected step. The original job,
tool pins, inputs, output bounds, workspace budget and step timeouts are unchanged.
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
