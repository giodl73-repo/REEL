# Plan an episode from logical Tig component IDs

```powershell
reel-tig-scene --profile PRIVATE_SOURCE plan-episode PRIVATE_REQUEST NEW_PRIVATE_PLAN
```

This command captures one immutable Tig source view and resolves a declarative
episode composition. It reads the exact native episode JSON and one named conform
stream through the protected metadata reader. Movies and scene source payloads
are not opened. It neither renders nor substitutes a generated path manifest for
the existing conform renderer's upstream checks.

The small request has schema `reel.tig-episode-plan-request.experimental-1`, an
`assembly_id`, `assembly_definition`, `episode_definition`, `scene_definitions`
and `component_definitions`. Definition roles must be distinct; all exact definition
pins must also be admitted in the existing private source profile. Every role requires
an explicit selected-object payload selector; a stream selector cannot silently
be reinterpreted as a current master or native source. Each definition list has
1–32 entries. Request JSON is at most 64 KiB and rejects duplicate/unknown fields.

The assembly is an ordinary `.tig` instance with `selected: null` and these
authored fields, governed by a project definition:

```json
{
  "episode": {"id": "episodes/example/source"},
  "language": "es",
  "conform_slot": "episode-conform.es.json",
  "components": {
    "opening": {"id": "episodes/example/opening/es"},
    "scene-001": {"id": "scenes/001/master/es"}
  }
}
```

These are references to current independently versioned output records, not a
second set of byte selections. Updating a master changes its own record; the
assembly file stays unchanged. Order comes from the exact native conform, never
map order, IDs or filenames. Every conform segment must map exactly once, with no
extra keys. There are at most 128 segments. Scene order must agree with the selected
native episode JSON; its declared `authored.scene_ids` maps each native scene to a
logical scene record. Those records must have the admitted scene definitions and
matching native `scene_id`/`episode_id` fields.

Each output has `language`, `source_segment_id` and `component_kind` fields, as
emitted by the conform-component registrar. A scene output also has `scene: {id}`;
an episode-presentation output has `episode: {id}`. They must match the declared
source scene/episode, exact requested language and native segment kind. There is
no fallback to another language, historical stream or status label.

The new plan captures source checkpoint, exact assembly/episode/component record
pins, selected native episode/conform pins, current master selections and original
baseline master pins in order. Each scene's current source record and selected
source payload are captured too. Changed master selections and unique output bytes
are counted, with duplicate digests deduplicated and contradictory sizes refused.
Those bytes describe selected component storage, not working/render scratch or
an estimate of the final encoded deliverable. The plan is bounded to 4 MiB and
requires a new absolute destination file with an existing parent. No overwrite
occurs; a failed write can leave a partial private plan that needs inspection.
Shareable stdout contains counts and flags only; the full plan remains private.

All plans say `executable: false`, `production_currency_verified: false`,
`payloads_verified: false` and `authorizes_eviction: false`. A master matching its
baseline says nothing about whether its inputs changed. A changed source-scene
pin does not manufacture a producer stamp. Current-source production, historical
producer evidence and transitive leaf-input changes require the full production
graph/recipe checks; unchanged scene-file pins alone cannot prove currency. Historical
retained-output reuse, native presentation/template rechecks,
decoded equality, rendering, delivery and custody remain distinct obligations.
The subsequent executor must preserve the existing conform renderer's checks.

## Current scene correspondence before execution capture

With `tig-publication`, `check_current_scene` accepts an actual current native
`JobPlan` and an independently replayed original producer claim. It binds the
current selected output under its exact definition, native scene ID, segment kind
and language; compares the current recipe with the original checked claim; and
returns a non-deserializable `CurrentScene` capability only on correspondence.
Current job checkpoint and source-node pins must belong to the caller's exact view.
No primary or output body is opened by this check. Status labels grant nothing.

`require_current_scenes` replans the assembly in that view and requires exactly
one matching capability for every native scene. Missing/extra identities, changed
source checkpoints, selected subjects or record pins refuse. Native production
definition sets must remain disjoint from the admitted component-output types:
component masters cannot become primary/control dependencies by relabeling them.
The returned logical plan retains its false execution/production/byte/eviction
flags. Presentation currency, byte checks and complete job admission remain open.

Source edits may still merge with deferred renders according to project policy.
These checks are an assembly/execution prerequisite, not a requirement to render
every authoring commit. Metadata-only status changes can retain a matching native
recipe; changing a consumed cel or cue requires new scene correspondence. Old
explicit source snapshots can still replay their own original version.

## Protected direct-native assembly and output publication

The `tig-publication` library path adds capabilities beyond the metadata-only
planning CLI. These do not change the planner's false execution/eviction flags.

```text
one source snapshot + exact current scene correspondence
       |
       +--> protect child jobs (independent source fences)
       +--> CompositionJobPlan -> freeze -> reopen -> protect parent
                                             |
protected parent/scene control trees + primary/output/runtime readers
                                             |
                    ProtectedComposition::assemble
                                             |
                         RenderedComposition
                                             |
explicit admitted output subjects + complete publication budget
                                             |
                    parent.publish_delivery
                                             |
                        PublishedComposition
                                             |
            plan.freeze_outputs -> finish provider -> reopen
                                             |
                     plan.readback_outputs
```

`ParentControls` holds the exact conform and its top-level metadata from existing
byte providers. `ControlTree` uses the existing Tig tree format for each native
scene's named job/contract/production/semantic controls. `CompositionFiles`
provides those controls and the separate primary/output readers. Assembly binds
the conform to its plan, uses its admitted runtime and checks ownership before
and after the full native operation. Local control files cannot rescue an
explicit provider refusal. The generated integration fixture makes its entire
source checkout unavailable during assembly.

The sealed result rechecks receipt and all retained output hashes. It can assert
unsigned correspondence for explicit admitted asset subjects. Complete
publication preflights provider capability and deduplicated object/byte budgets
before writes, then acknowledges outputs, receipt and claim. Keep the same
operation/owner identity for an exact interrupted retry. A changed render needs
a new operation. Publication is local byte retention, not creative approval or
Google Drive/YouTube delivery.

Freeze and independently reopen the published resource set before proposing an
owner transition. It retains the original runtime/composition/evidence roles and
adds the checked outputs, receipt and claim. It still reports incomplete claim
coverage: full child primary/control closures belong to the independent child
owners. Keep them active; no child retirement or cache eviction is authorized.

The path currently supports direct native scene components, not actual retained
opening/credits presentation eligibility. This low-level assembly primitive is
not a capacity scheduler; production execution needs the existing bounded job
runner. Parent native CLI/scheduler integration, full child-retention handoff,
generated-input freshness and actual project adoption remain separate work.
See [the output publication review](reviews/2026-10-29-episode-output-publication.md).
