# Run an already enrolled job

```text
reel-tig-job run-frozen PRIVATE_PROFILE
```

This command replays an already enrolled generic job through the existing managed
runner. An operator must already have frozen, finished, reopened and owner-adopted
its job context. It neither enrolls a job nor initializes stores. An authored
control edit can use this path without claiming to be a qualified native render.

The private `reel.tig-frozen-job-profile.experimental-1` JSON contains:

| Field | Contract |
| --- | --- |
| `project` | Existing absolute project directory |
| `metadata_root`, `metadata_minimum` | Existing store and independently trusted minimum |
| `source_context`, `source_checkpoint` | Exact frozen production context and expected source checkpoint |
| `profiles` | Caller-admitted graph profiles, definition pins and payload meaning |
| `consumption` | Definition ID, exact definition Object and consumed authored `fields` for each type |
| `operation` | Recipe operation: name, semantics digest, producer and parameter Objects |
| `runtime` | Exact runtime Object map; `request` must pin the raw manifest |
| `job_context` | Exact already enrolled job Selection |
| `manifest` | Absolute FileRef to the exact generic runner manifest |
| `cache` | Existing Config, including explicit shared coordination root |
| `journal_root`, `journal_minimum` | Existing source journal and trusted minimum |
| `source_id`, `owner`, `expected_revision` | Exact current source authority |
| `retain_operation` | Unique operation key for the result handoff |

These profiles are trusted local operator admission, not instructions obtained
from an asset's status. Keep them outside Git and public logs. Profile intake is
128 KiB and manifest intake 256 KiB. Existing bounded Tig APIs independently
replay the source context and job admission. Checkpoint, recipe parameters,
request pin and full resource selection must match before execution. Claimed
outputs and extra composition/evidence contexts refuse through the existing
runner's input-only contract.

The runner checks owner, revision and current selection under its scoped source
fence, admits capacity, verifies input/tool pins, executes contained children and
retains/readbacks declared outputs before retiring owned scratch. Other sessions
keep their independent fences and admission rules. No global cache lock, new
provider or fallback cache is introduced.

Profile and manifest handles exclude write/delete sharing on Windows through
execution. Other platforms require a caller-owned stable input namespace; this
is not a sandbox against hostile path/ancestor replacement. The worker and
manifest remain trusted code. Fixed failures omit private paths, hashes, IDs and
text. Successful runner output contains private local details and belongs in the
operator's private workflow. A completed failed job returns a failing process
exit status, retaining the ordinary failed-job lifecycle.

This command does not create a Git proposal or transfer source ownership. Retain
both control and candidate record as outputs, reopen their durable result root,
then use the existing proposal API on those exact retained bytes. Keep the result
owner until explicit consumer adoption or retirement supersedes it. Generic job
success has `production_claim: null` and `creative_approval: false`.

See [control candidates](tig-control-edit.md) for the worker contract. The
synthetic managed fixture runs this command before the existing one-record Git
proposal check. Actual project enrollment and consumer adoption remain separate.
