# Streaming dependency expansion and recipes

Explicit large retained dependency budgets now travel through the ordinary
late-bound expansion and recipe paths. This connects upstream-inputs v3 to
authoring and scheduling; it does not add episode partitioning or qualify
consumer-scale native execution.

## Authored contracts

`TaskTemplate` has an optional `streaming` field with the existing
`reel.workflow-upstream-streaming.v1` schema, total `input_bytes` and
`input_object_bytes`. A direct template with this field uses
`reel.workflow-expansion-template.v2`. V1 requires the field to be absent;
v2 requires it to be present. Numeric upstream limits are validated before
observing parents, and a streaming template must actually consume parent inputs.

Expansion resolves declared output/worker-receipt selectors from verified
original retained observations, constructs upstream-inputs v3, then rechecks
the complete exact dependency binding and byte/object allowances before export.
The native source-freezing layer continues to capture the exact returned
bindings and original parent evidence. No packet or native declaration needs
manual patching to introduce the streaming allowance.

`reel.workflow-recipe.v2` permits per-step streaming settings and mixed bounded
legacy steps. Recipe v1 refuses streaming steps. The scheduler emits the correct
template version for each step. The resumable native runner reconstructs that
same version when checking a pending frozen expansion. Recipe/config/intent
pins and existing-step comparison still refuse changed budgets under an
already started identity. No old operation, native runtime or task packet is
silently upgraded.

Absent fields are omitted from serialization. Legacy task templates, recipe
digests and generated packet bytes retain their original representation. Existing
task/actor/changed-path/projection/reader/film limits remain independent. There
is no implicit partitioning, payload discovery, new pool, slot allocation or
lease expiry. `expand-plan` and `schedule-recipe` export private planning
controls; admission and execution remain separate.

## Tests and qualification scope

New ordinary tests cover a declared 3,600,000,001-byte parent artifact, exact
result fingerprints, upstream v3 output, byte/object refusal, budget-dependent
child keys, template/recipe version pairing, no-parent refusal, existing-step
budget changes and unchanged absent-field legacy round trips. These are
synthetic contract observations, not native large-object availability.

An opt-in fixture invokes an explicitly frozen native CLI against an existing
retained synthetic comic parent. It exports both a v2 expansion and a v2 recipe
schedule, compares their exact child packets/dependencies, requires their
no-admission reports and compares the actual Index checkpoint before and after.
It writes only new owned private controls/evidence.

The fixture passed in 66.06 seconds (66.80 seconds including process launch).
Both actual CLI commands produced the same exact child packet and original
dependency digest, with v2 templates and v3 upstream bindings. Their selected
parent artifact was the existing 10,138-byte synthetic reading PNG, not a
multi-GB intermediate. The actual Index checkpoint remained unchanged at
sequence 19. No child was admitted, no provider/producer ran and no task was
completed or adopted.

Source: `2a56aa27fb769adefb849272ab5398b2b8ce6703`; all 1,944 tracked files
are archived and individually hash/length compared against the working tree.
Frozen CLI SHA256
`9257fd79948479a32b76fb762d10274d8f86d20ed900676723ab6dc893a68175`,
38,849,024 bytes. Frozen test tool SHA256
`0c25c7348e5f4702d682b977adb6495b028f27b3d03c1a4bb454aaab22bb44d7`,
43,981,312 bytes. All 118 workflow, 84 standard native and 31 HTTP tests pass,
with all 247 SDK pins and strict all-target Clippy for workflow and native
with/without film. The standard suite excludes its 64 ignored native fixtures;
only the named read-only CLI fixture ran separately. `git diff --check` passes.

Private certificate:
`F:/src/tig-dev-temp/reel-streaming-expansion-20261119-v1/qualified-tools.json`,
SHA256 `18a9294876bd90e6134e19eb8917142bf5ec9c52a78abc9c750df017968ac6a0`.
It binds the frozen source/tools, original start/outcome, exact exported controls,
CLI outcome records, unchanged Index checkpoints and all three test logs.
Qualification session 33930 and regression session 24447 terminated successfully;
there is no pending child lifecycle to resume or re-admit.

Automatic authoring of a complete bounded episode composition, real multi-GB
parent/child admission and retained handoff, large film dispatch, interrupted
consumer-scale recovery and project adoption/rollback remain open. The producer
rights/provenance checklist keeps original source lineage and exact approval
subjects separate from technical planning. No actual S4/S5 job, media, selected
recording, creative approval or delivery is changed here.
