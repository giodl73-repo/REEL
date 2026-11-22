# Candidate scene controls and affected-film planning

Two explicit metadata commands connect returned image candidates to new scene
controls and a proposed language rebuild plan. Neither command selects assets,
transfers approval, admits native jobs, calls a provider or renders media.

`reel scene-delivery-derive-still JOB --job-sha256 SHA --replacement SPEC
--replacement-sha256 SHA --output-path NEW_JOB` verifies both exact metadata
files and creates a new job exclusively. The spec has schema
`reel.scene-still-replacement.v1`, explicit `attachments`, and exact `expected`
and `replacement` file references (`path`, `sha256`, `bytes`). Only the named
still sources change. Missing, duplicate, stale and video targets refuse.
The original contract, sample/frame timing, neighboring pictures, audio and
camera controls remain unchanged. Retained crop/motion or a post-compose camera
sets `geometry_review_required`; those coordinates still need drawing review.
Run separately for each language's original job. Never derive an English clock
or recording from the Spanish job.

`reel-workflow prepare-film-rebuild REQUEST SHA256 NEW_DIRECTORY` exports
`prepared.json`, `plan.json` and exact per-render upstream binding documents.
The request schema is `reel.workflow-film-rebuild.v1`. Supply the original
`baseline` plan and `old_subject`, the returned image `packet` and `result`,
its output name, a map of control parents and an explicit `consumers` list.
Each consumer identifies its task, control task/output, new picture alias,
baseline-job alias and optional newly generated native input declaration pin.
The integration test `crates/reel-workflow/tests/film_rebuild.rs` is an executable
synthetic request example.

All direct consumers of the exact old image must be covered. Supported direct
consumers are whole-scene FilmRender tasks without an existing upstream binding.
Controls must name the exact old job and new image, match the consumer language
and bind the exact image result. Manual text/control results remain handoffs;
their declarations do not prove deterministic tool execution or validate their
JSON as scene controls. The scene-control command above performs that transform.
The planner itself reads only workflow metadata, not picture or job payloads.

New render tasks retain original inputs as baseline evidence and append the new
image/job with exact parent bindings. Supplied new native pins require separate
native validation; omitted pins are listed in `native_binding_required`. Old
native pins are never copied. Unaffected tasks are preserved. Affected downstream
assemblies are listed in `pending_consumers` and withheld until new exact render
results can replace their former inputs.

Bounds remain unchanged. Existing upstream closures, unsupported direct tools,
colliding logical IDs and incomplete consumer sets refuse rather than silently
discard history. This first adapter does not merge prior upstream closures,
authenticate the caller's baseline against a retained render, verify selection,
or execute the full repair loop. Project authority, native binding/admission,
protected dispatch and downstream result rebinding remain explicit subsequent
work. `selection_verified`, `approvals_transferred` and `consumers_rebuilt` are
false. No production profile adoption or episode-scale qualification is implied.
