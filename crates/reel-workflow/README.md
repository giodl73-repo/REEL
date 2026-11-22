# Workflow core and manual exchange (W1)

This crate provides project-neutral task DAGs, snapshot-bound packets, typed
results, bounded streaming byte checks and a local manual prompt/result loop.
It is the first code slice of the [workflow PR plan](../../docs/workflow-pr-plan.md).
It is not a production film runtime or an adoption of any existing production.

The [existing-film inspection adapter](../../docs/workflow-film-inspection.md)
adds read-only sidecars for pinned native v2 scene metadata. It reuses
`reel-assembly`, with no native SDK or media-renderer dependency.

Run the synthetic example from the repository root:

```text
cargo run -p reel-workflow -- capabilities
cargo run -p reel-workflow -- check-plan examples/workflow-manual/plan.json
cargo run -p reel-workflow -- export-work examples/workflow-manual/plan.json cel-note attempt-1 examples/workflow-manual/dependencies.json NEW_WORK_DIRECTORY
```

The new directory contains `packet.json`, `prompt.txt` and
`response-contract.json`. Run the prompt yourself using an authorized tool and
save the response locally. Create a private JSON file mapping output names to
absolute file paths, for example `{"note":"/absolute/path/response.txt"}` (use
escaped Windows paths or forward slashes on Windows). Then run:

```text
cargo run -p reel-workflow -- capture-work NEW_WORK_DIRECTORY attempt-1 FILE_BINDINGS_JSON NEW_CANDIDATE_DIRECTORY
```

Capture hashes the actual bytes using a fixed 64 KiB read buffer, checks the
declared size bounds, and writes a result envelope and candidate control files.
Output payloads stay at the supplied locations; this is not a cache or retained
storage. The media type is the declared contract, not a format or semantic
inspection. Returned HTML/CSS/SVG is opaque data and is never executed here.

For callers already holding a result envelope, `accept-work WORK_DIRECTORY
ATTEMPT RESULT_JSON FILE_BINDINGS_JSON NEW_CANDIDATE_DIRECTORY` checks the same
bytes and original task/attempt binding. Exact replay rechecks the payloads and
does not rewrite candidate files. A conflicting result, changed work package,
partial candidate directory or unexpected entry stops and preserves local state.
Do not treat a failed write as permission to restart an external effect.

`check-result PACKET RESULT` checks metadata only. Adding `FILE_BINDINGS_JSON`
also checks bytes. `export-task PLAN TASK NEW_PACKET` exports a planning packet
for a task with no dependencies; `export-work` accepts explicit dependency result
metadata. Neither command acquires input media or supplies input byte custody.

Plans and packets preserve authored language tags, prompts, input identities,
output contracts and exact snapshots. They do not yet implement language-local
conform, cue recording, cel timing, rendering, issue filing or affected rebuilds.
Those remain separate PRs. Native storage and API/proxy providers are absent
from this build; no comic renderer, SDK copy or network client is required.

Candidate files contain private context and absolute local paths. Keep them in
private storage outside source control. CLI reports contain bounded status/error
codes rather than prompts, reply text, paths or asset identities. Byte validation
does not authenticate the caller's claimed provider provenance, establish retained
ownership, complete a production task, select a candidate or grant human approval.
Reverify externally held bytes before later use.

The `handoff` library separately defines an append-only journal and abstract
managed lifecycle interface. Its tests use fake lifecycle adapters to test
ordering, stale-generation fencing and reconciliation of the original operation.
They do not qualify a native storage implementation. `check-handoff DIRECTORY`
inspects only local journal state; `cancel-handoff DIRECTORY` records local
cancellation where allowed and releases no native protection. There is no CLI
native-admission/import command in W1. Storage qualification follows W3a.

Control JSON is limited to 1 MiB, authored prompts to 64 KiB and each plan to
128 tasks. These are control/admission bounds, not universal film/media limits.
Each output declares its own byte ceiling. Interrupted multi-file writes are
preserved for diagnosis rather than silently repaired. File handles prevent
sharing writes during checks on Windows; portable checks do not promise an
immutable snapshot against concurrent writers on other platforms, nor does any
candidate record make mutable external paths durable.

Validation:

```text
cargo clippy -p reel-publication -p reel-workflow --all-targets --locked -- -D warnings
cargo test -p reel-publication -p reel-workflow --all-targets --locked
```


`input::InputReader` is a small generic named-Artifact read seam for optional
storage adapters. It adds no media/rendering or native SDK dependency to this
crate. The isolated [public git-asset adapter](../reel-workflow-git-asset/README.md)
supplies exact snapshot preparation and the existing handoff Lifecycle over a
pinned public SDK. Construction is no job admission or production activation.
