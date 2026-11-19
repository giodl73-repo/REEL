# File-backed movie capture and worker completion

The owned movie producer now captures media as verified `FileRef` values behind
live ordinary-file/namespace guards. It retains only execution controls and the
small worker receipt as byte buffers. `movie::generate_files` exposes that
candidate port to Rust callers with live admitted inputs and an explicitly
owned/capacity-admitted fresh workspace. Its returned `FileGenerated` must stay
live through use/import; file paths alone grant no authority or custody.

The existing buffered `movie::generate` and `film_process::render`/`recover`
interfaces remain explicit bounded compatibility APIs. The automatic worker and
the contained renderer's final collection use the file-backed path. No complete
movie is cloned into the worker's control-byte map on that path.

The worker records the already-produced files in its existing result manifest.
It checks exact canonical locations, rejects buffer/file aliases and includes
every output identity in the same declared byte/count budget. Staging scans and
existing-file verification hash fixed 64 KiB chunks. An in-place file is verified
at its final owned path; it is neither copied into a part nor silently repaired.
Missing/corrupt files refuse before completion. Marker bytes, manifest/intent
identity, source claims, preserved capacity, original result/attempt and native
retained-owner acknowledgement retain their existing separate gates.

The CLI also accepts explicitly pinned media programs during standalone worker
preparation (the former form without programs remains unchanged):

```text
reel-workflow-native prepare-worker PLAN RUN_BINDINGS BATCH BATCH_SHA256 TASK_ID STAGING_PROFILE PROGRAMS_JSON NEW_TICKET --apply
```

Only the existing declared capability/program validator admits those programs.
This does not expose arbitrary command execution or treat returned code as a
renderer.

Two filesystem component tests verify interruption/resume with no media buffer
or duplicate part, held-file mutation refusal, and missing/corrupt file refusal
without completion or replacement. Their initial test setup reused an existing
workspace operation and correctly refused; distinct explicit fixture operation
identities fixed the harness without relaxing ownership checks.

## Native qualification to execute

The bounded native fixture uses existing synthetic movie inputs and explicitly
closes the retained automatic-resolution source slot into retained coverage,
then reuses the same Index/slot for a distinct new movie task/runtime. The v1
ready-batch protocol still requires its original bounded input-staging ticket;
this small fixture stages its inputs and does not qualify a large-input borrowed
dispatch mode. It creates an owned worker ticket and renders once through
`generate_files`. At a deliberate
controller boundary, render request/outcome exist but worker receipt/completion
do not. A separate original-ticket worker process must adopt those exact completed
outputs, retain/reopen them and replay without render-control/timestamp or claim
changes. Review IDs must match the original synthetic movie. Previous resolution
history and the original movie's separate claims must remain intact.

Source/Index work, activation, ticket, tool pins, returned bytes and scratch are
preserved on failure. Do not rerun this effectful fixture fresh after acceptance;
reconcile the original handles. Its actual results are recorded after execution.
This is a completed-render/controller-boundary fixture, not proof of every
mid-render crash, lost-evidence or episode-scale recovery case.

All v1 input/output/workspace/time/actor/path limits remain. This capture work
does not qualify the S5 600-resource / 7.94 GB consumer, multi-GB masters, owned
real-project adoption, translation or creative approval, selection, scratch
retirement or release. Explicit large-job budgets, bounded composition and
cross-revision task snapshot/adoption semantics remain required. Producer-applied
editor and rights/provenance checks preserve original IDs, approvals, private
locators, selected-take reuse and technical/creative authority separation.
