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
It writes only new owned private controls/evidence. The result and frozen
source/tool pins must be recorded before claiming that qualification.

Automatic authoring of a complete bounded episode composition, real multi-GB
parent/child admission and retained handoff, large film dispatch, interrupted
consumer-scale recovery and project adoption/rollback remain open. The producer
rights/provenance checklist keeps original source lineage and exact approval
subjects separate from technical planning. No actual S4/S5 job, media, selected
recording, creative approval or delivery is changed here.
