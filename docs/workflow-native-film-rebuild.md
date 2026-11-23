# Native affected-film binding

`reel-workflow-native prepare-film-rebuild REQUEST SHA256 NEW_DIRECTORY`
connects the portable affected-language plan to native source profiles. It reads
bounded private metadata and creates an exclusive metadata package. It does not
admit a child, publish bytes, reserve workspace capacity, render, select assets,
adopt a profile or release parent history.

The request schema is `reel.workflow-native-film-rebuild.v1`:

- `rebuild`: the complete portable film-rebuild request. Its consumer native
  pins must be absent; this binder computes them.
- `baselines`: one entry per affected render. Each supplies its original
  `packet`, original native `source` and explicit new `budget`.
- `parents`: `reel.workflow-native-parents.v1` controls for the exact image and
  control packets/results named by the portable request.

The binder compares each original packet with the baseline plan and resolves
its exact native declaration against the original Git commit. Identity, record,
selected stream and annex leaf must agree; matching payload bytes alone do not
establish this binding. The original Git inputs remain baseline evidence.

Each new source profile adds frozen origins for only that render's required
parents. It computes the v3 native declaration pin, builds the new packet with
exact dependency results, and resolves the entire composite metadata closure.
Other languages' control origins are excluded. Original snapshots, runtime pins,
requests and the producing project's selection remain unchanged.

The package contains `bound.json` and per-render `*-packet.json`,
`*-source.json`, and `*-parents.json`. The command writes `export.json` last
and returns its SHA-256 as `export_sha256`. Run
`verify-film-rebuild-export DIRECTORY REQUEST_SHA256 EXPORT_SHA256` to check
the original request digest and each listed file's length and SHA-256 before
dispatch. Each file is at most 1 MiB and the complete metadata
export is at most 16 MiB. Partial and existing packages are preserved.
These are private machine controls, not shareable reports.

`dispatch-film-rebuild DISPATCH SHA256 --apply` accepts
`reel.workflow-film-rebuild-dispatch.v1` with `export_directory`,
`rebuild_request_sha256`, `export_sha256`, and an `editions` map keyed by
the exported render task IDs. Each edition supplies `request`,
`request_sha256`, `ticket`, `ticket_sha256`, and `action` (`run`, `resume`,
or `verify`). Prepare and persist the owned requests and tickets first.
The dispatcher checks every edition's exported packet, source and ticket
binding before it starts the first render. It uses the export's task order
and stops at the first failure. After interruption, preserve the original
requests and tickets, resume an interrupted edition, verify a completed
edition, and run any edition that has yet to start.

Each successful edition reports `retained_scene` with the task key, attempt,
output pins, worker receipt and retained acknowledgement. Use these pins as
inputs to the episode segment binding. The episode worker still independently
checks the selected master against the current language and scene controls.

Use the existing `preview-upstream-admission` and explicit
`accept-upstream-admission` with that render's files and an owned admission
profile/attempt. Those operations reopen actual retained parents and acquire
the child's own positive claims. Serialized parent acknowledgements and this
metadata export cannot substitute for live custody. Original-operation recovery
and conservative waiting/close rules remain unchanged. The binder's
`live_parent_custody_verified` and `child_admitted` fields are false.

## Check actual derived controls before rendering

With the Windows `film` feature, an owned film CLI request can additionally
carry `control_derivation`. Old requests omit this field and serialize unchanged.
The value has schema `reel.workflow-film-control-derivation.v1`, an exact
`original_job` artifact and a `replacement` still-replacement specification.

Before workspace reservation and again before a new render, the CLI reads both
jobs through the child's own claims. It verifies the original/new picture
aliases against declared inputs and runs the shared deterministic still-control
transform. The actual candidate job must equal that derived job. Changes to
selected takes, language clock, duration, other pictures or camera controls
refuse before rendering. The original job must remain an explicit input.
Unchanged geometry is preserved and reported in the owned workspace's pinned
intent for review, never newly approved.

This check is opt-in for declared still repairs; general authored films continue
to use their existing controls. It verifies transformation and input lineage,
not creative suitability, selection authority or human viewing/listening.

Remaining integration: authenticated original retained-render adoption, protected
control-task production, project-owned selection policy, automatic per-consumer
dispatch and downstream assembly rebinding. Existing upstream closures still
require explicit merge support. No episode-scale capacity increase is included.
