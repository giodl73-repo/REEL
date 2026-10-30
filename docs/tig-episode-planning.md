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

The scene-only constructor retains its explicit scene coverage requirement.
`CompositionJobPlan::from_current_components` additionally accepts sealed
`VerifiedPresentation` values from the native presentation verification job.
The selected output record, hash, size and captured source must match each
presentation occurrence exactly. Component-output definitions cannot be admitted
as its source/control/primary definitions. A status label or an arbitrary movie
pin cannot construct this capability.

The plan borrows the verified presentation values, retaining their source,
runtime, primary and output guards. It records each exact child job selection
and comparison document and explicitly includes the child's complete declared
resource set in its evidence obligations. It does not rely on an opaque receipt
to supply a hidden closure. Presentation movies remain Composition dependencies;
they never become Primary inputs. Scene child obligations remain separately
protected until complete handoff is implemented.

Parent adoption and each native action check all presentation owners as well
as the scene and parent owners. A withdrawn presentation invalidates its parent
before an action; it does not invalidate an independent source job. A parent
cannot replace a child by adopting the same source identity. Resource limits
cover the union before any parent journal change. A presentation-only composition
can have an empty scene-owner set, while still requiring exact nonempty native
component coverage and its actual presentation guards.

`from_current_components` compiles effective native presentation controls from
the captured component records and their sealed proofs. The pinned conform
provides order and layout; its older presentation movie/receipt references are
replaced in memory. Historical receipts are not rewritten. Generated controls,
technical proof declarations and complete top-level metadata bodies enter the
frozen job's evidence and are checked again on readback. They do not enter Git.

The ordinary `assemble` route can consume that compiled plan. Native selected
movie paths come from the proof's retained byte guards, with no caller substitute
or historical adoption directory. The native kernel validates catalog, ready
state, binding scope, role order, frame/sample counts and final decoded/timestamp
correspondence. Native reference evidence retains its own schema and is not
represented as an original producer receipt. The legacy provider-based conform
entry points remain separate.

For a changed scene, `CurrentSceneControls::compile` recompiles metadata under
the matched current native job and binds it to the selected guarded delivery
receipt. `from_current_component_controls` consumes these sealed controls along
with current scene correspondence and optional presentations. It replaces the
scene's job, semantic scope, receipt and master references in memory, retaining
only the four native render controls and exact receipt. Assembly supplies those
frozen controls directly and rejects caller replacement. The original conform
and producer evidence remain immutable. The existing constructors continue
refusing changed scenes without this control coverage.

Native presentation invocations with content overrides or editable source-text units also require
their current content-binding contract; they cannot silently reuse an unrelated
render. Those extensions remain open. The native parent compiler currently
handles source-owned presentation content with empty invocation overrides.

This low-level assembly primitive is not a capacity scheduler; production
execution needs the existing bounded job runner. CLI/scheduler integration,
full child-retention handoff, generated-input freshness
and actual project adoption remain separate work.
See [the output publication review](reviews/2026-10-29-episode-output-publication.md).
See [current scene controls](reviews/2026-10-30-current-scene-parent-controls.md)
for the sparse-edit fixture and exact qualification requirements.

For legacy masters without native produced correspondence, the separate
[primary-reference comparator](reviews/2026-10-30-scene-primary-reference.md)
reconstructs native pixels and PCM independently of old receipts, using video
pipes and bounded audio/font staging. It is a technical observation primitive;
current-source-owned Tig proof and parent admission remain separate integration.

## Retained presentation controls without a checkout

`presentation_adopt::build_with_control_files` takes an exact manifest `FileRef`
and separate control and composition-source providers. `Selection` names the
retained manifest, master and receipt for `check_with_control_files` or
`check_cached_with_control_files`. All metadata is bounded to 4 MiB per document
and checked against its exact declared hash and size. Virtual document names do
not resolve relative to physical CAS paths. Missing subjects, changed pins or
missing runtime programs refuse without a local-file or PATH fallback.

`episode_conform::build_with_component_controls` combines parent controls,
direct-native scene controls and adopted-presentation controls. Coverage must
match the selected component kinds exactly. Selected presentation masters are
composition dependencies; their source movies do not become primary inputs.
Ordered-unit source-text evidence uses the same explicit metadata provider.
This entry point performs full presentation checks, without implicitly writing
a verification cache. Explicit verification-cache reuse rechecks every subject,
validator and tool before accepting an exact previous successful check.

```text
Tig-selected presentation manifest + controls     retained source movie
                  |                                      |
          control provider                       composition provider
                  +-----------------+--------------------+
                                    |
                       full decoded equivalence check
                                    |
                  retained master + receipt provider
                                    |
              episode parent / ordered component composition
```

This closes the local-control dependency of the native presentation checker.
It does not establish the source movie's producer lineage, creative approval,
complete primary closure, child-owner handoff or eviction authority. The sealed
Tig composition admission path separately requires the guarded native
presentation proof described above before it can accept retained openings and
credits. Low-level conform is not a storage-capacity scheduler.

## Producer checklist findings for presentation admission

- Editing: occurrence order stays in the admitted native conform. Exact output
  scope and current-source comparisons cannot substitute a different occurrence.
  Current presentation controls are generated from the selected output and its
  sealed proof; changed scenes retain their explicit native-control gate.
- Sound: the native proof binds matched PCM as well as both picture hashes;
  retained ownership covers the primary recording through use.
- Provenance: comparison evidence explicitly records that it is not approval or
  an original-producer assertion. Historical receipt hashes remain unchanged.
- Storage: flatten exact child obligations into retained evidence, preserve
  independently owned scene obligations, and retain live guards. This does not
  authorize retirement or eviction, prove independent backups or establish
  complete font/environment isolation.

These are producer-applied checklist findings, not independent or human review.
The parent fixture now uses a real ordered-unit layout and performs native
assembly over a genuine current presentation proof with source records offline.
Its authored conform deliberately names a stale movie; the compiler substitutes
the current selection without reading that obsolete movie. The fixture also
checks missing generated metadata and a withdrawn child owner. It does not claim
a real mixed episode's complete native render, full scene-control conversion or
cutover.

Prior PR245 qualification: locked all-target/all-feature Clippy
passes with warnings denied; 216 library tests pass (nine explicitly ignored),
all 13 episode-planning tests pass, and all eight presentation-source tests pass.
The stale-conform counterexample refuses before execution. The explicit complete
fake-episode lifecycle also passes (93.60 seconds), including the changed-scene
rebuild, Git history, retained outputs and verified synthetic cache eviction.
The current control-compiler extension requires its own final tests and exact
hosted qualification before adoption.

For retained scenes without an original native Tig claim, the
[owned scene-reference path](reviews/2026-10-30-owned-scene-reference.md) freezes
current controls and selected-output resources, reopens them, and retains a
source owner plus byte guards through independent picture/PCM comparison.
Its sealed `VerifiedScene` is separate from historical recipe correspondence.
The [verified-scene parent path](reviews/2026-10-30-reference-scene-parent.md)
now explicitly consumes these proofs, borrows their guards and retains complete
declared child obligations through checked native assembly. Its tiny episode
fixture replaces one cel without editing the authored conform, with old jobs
and the obsolete movie unavailable. Actual legacy episode acceptance, bounded
orchestration and runtime cutover remain required.
