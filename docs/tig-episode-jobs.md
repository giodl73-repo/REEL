# Bounded episode execution

`reel-tig-job run-episode PRIVATE_PROFILE.json` is the explicit episode route over
an already captured source and existing caches, metadata and source-journal
stores. It performs no cache or daemon setup. Keep the machine profile, raw
reports and runtime receipts outside Git.

## Verify retained components without assembling an episode

```text
reel-tig-job verify-episode PRIVATE_VERIFICATION_JOB_PROFILE
```

Set `inputs.purpose` to `verify-components` in the otherwise normal episode job
profile. `run-episode` requires the default `assemble` purpose; the two commands
refuse mismatched profiles before opening stores. Verification profiles must not
declare output-record mappings or a Git handoff. The ordinary episode output
contract remains part of the source and its resource plan, but the contained
verification request declares only `worker.json` as its output.

This is an actual managed job: component availability checks, capacity admission,
process containment and source ownership still apply. The worker obtains the
same fresh scene/presentation primary-reference proofs as assembly and verifies
the protected parent and children again before reporting. It skips the parent
assembly call entirely. The result states the component count and verified
primary references, with `episode_assembled: false`,
`final_episode_output_verified: false`, no production claim and no approval.
Reference checking may use bounded temporary decoded/staged data; it does not
produce a new episode movie or promote any selected asset.

The verification purpose is pinned into the frozen resource tree. Retention and
recovery validate that pin and the distinct worker report schema. A successful
verification cannot be interpreted as a completed assembly; failed verification
remains failed after diagnostic retention and cleanup. The existing retained
context handoff keeps the full evidence before withdrawing temporary owners.
Use `finish-episode` with the unchanged verification profile for recovery; changing
its purpose refuses instead of changing the meaning of the retained result.
Older profiles and retained contexts without a purpose retain assembly semantics.

Scope: entry source `9c64941fb86278aef00e02b2a55ac62735278fcd`, Tig
WP-005/011/015/016 and REQ-001/003/006/007/010. Reuse the existing verification,
runner, source-journal and handoff mechanisms; no new proof engine/provider.
L0 format/lint; L1 purpose separation, output-contract refusal and immutable
mode binding; L2 native scene-only/mixed jobs, missing movie before adoption,
missing primary after adoption, result retention, owner cleanup, idempotent
recovery and changed-purpose refusal. Rights/provenance producer review must
keep component equivalence distinct from final-delivery verification and approval.

Local native verification passes both scene-only and mixed-presentation fixtures,
including missing-primary failure retention and changed-purpose recovery refusal.
The first run exposed a namespace mismatch: dependency obligations use
`input-evidence/`, while `evidence/` is for output-claim attachments. Recovery now
requires the exact marker in the input namespace; a focused regression rejects
missing, foreign or output-only markers. Hosted assembly compatibility and actual
project verification remain separate acceptance work.

## Prepare the complete resource plan before execution

```text
reel-tig-job prepare-episode PRIVATE_JOB_PROFILE NEW_PRIVATE_REPORT
```

This explicit operation uses the same episode job profile and complete child
planning/readback code as execution. It binds the declared runtime, compiles
small native controls and freezes the parent, child reference and runtime
resource set in existing metadata storage. Bounded coordinator preparation space
is reserved and released; this is a writing operation, not a read-only preview.
It never opens the source journal, adopts a job or starts a media producer.

The fresh, at-most-64-KiB report must use the preparation volume outside metadata,
source-journal and CAS object directories. It records the exact raw profile pin,
source checkpoint, frozen resource selection, recipe, runtime manifest, metadata
checkpoint, unique resource count/logical bytes and configured output/workspace/
scratch ceilings. The full resource list remains in the frozen metadata tree;
no expanding inventory is added to Git or stdout. Output ceilings and logical
resource bytes are not physical allocation estimates or promised render sizes.

Preparation does not verify selected component movies or primary media. A missing
movie can therefore be represented in a successfully prepared resource plan.
This establishes declared requirements, not availability or producer currency.
The report explicitly denies execution capacity reservation, root retention
enrollment, production and eviction authority. It is not an execution ticket or
lease. A later `run-episode` still repeats preparation, verifies component media,
checks current owner/revision state, reserves execution capacity and obtains fresh
guarded primary-reference proofs. A prepared report never bypasses those checks.

Scope controls: entry source `6b403aa1ec6730b826f265796092355af6fc1d82`;
Tig WP-005/011/015/016, REQ-001/003/006/007/010, existing native source/job and
resource interfaces. Allowed scope is shared episode metadata orchestration and
private reporting. L0: format/lint; L1: profile/path/budget refusal; L2: synthetic
real-Git scene-only and mixed-presentation CLI fixtures with Git and a selected
movie offline, no source-journal creation/advance, verified metadata readback,
no-replace reporting and unchanged execution refusal for missing movies.
Implementation, test results, hosted qualification and actual project adoption
remain distinct. Existing rights/provenance review requires the explicit false
approval/currency/production flags; technical preparation grants no selection.

Local producer verification passes both focused native preparation fixtures,
102 regular Tig-input regressions (19 explicit native fixtures ignored), strict
Clippy for the library/job CLI/Tig-input tests, formatting and diff checks.
The first full native regression reached retained output/proposal handling but
failed because the local fixture lacked `REEL_TEST_TIG_PROPOSE`; that is not a
full-test pass. Full fixtures now require this binding before producing their
synthetic media. Hosted full workflow qualification and actual project resource
preparation remain separate from these local checks.

First capture a committed episode with
`reel-tig-job capture-context PRIVATE_CAPTURE_PROFILE.json NEW_FRAGMENT.json`.
Use capture schema `reel.tig-episode-capture-profile.experimental-1` and supply
`source` (the committed-source profile), the episode `request`, existing
`metadata_root`/`metadata_minimum`, `metadata_cache`, `preparation_cache` and
`cache`. Metadata and fragment output must share the explicitly admitted volume.
The command reserves bounded metadata/preparation capacity, freezes the episode
context, finishes/reopens the provider and independently restores the selection,
descriptor, requirements and archive before publishing a new private fragment.
It refuses to overwrite an existing fragment. It renders no media and adopts no
job; capture alone does not establish child-primary eligibility or byte custody.

The fragment supplies `source`, `context` and `request` for the job's `inputs`,
and the updated `metadata_root`/`metadata_minimum` for the job profile. Runtime,
component requests, owners, output contracts and budgets remain explicit job
admissions. The existing scene capture schema and compiled-context fragment
remain supported by the same command.

```text
captured source + admitted episode/component requests
                     |
      metadata-only complete input resource plan
                     |
     capacity admission + root source obligation
                     |
           contained trusted worker
                     |
        owned scene/presentation comparisons
                     |
      owned current parent + native assembly
                     |
     verified outputs -> existing CAS retention
```

The private profile schema is `reel.tig-episode-job.experimental-1`:

- `inputs` supplies one archive profile, its frozen parent `context`, the parent
  `request`, pinned `runtime`, and exact `scenes`/`presentations` coverage. Each
  component supplies its segment `id`, native `request`, `output_id` and explicit
  `output_definition`. A scene may set `compiled: true` for projected controls.
- `cache`, `preparation_cache`, `metadata_root`/`metadata_minimum` and
  `journal_root`/`journal_minimum` select already existing shared providers.
  Independent worktrees should share those providers while using distinct source
  identities and explicit operations.
- `source_id`, `owner`, `expected_revision` and `operation` admit the root source
  transition. Reusing an already completed revision is refused.
- `budget_bytes` covers the workspace, scratch and atomic result publication;
  `scratch_bytes` covers temporary compilation, audio and assembly files.
  `timeout_seconds` bounds the contained process tree. These are real runner
  admissions, not claims based on expected compression ratios.
  The coordinator declares the same per-request `step_timeout_limit_seconds`
  to the shared runner, up to its finite 24-hour maximum. Ordinary runner
  requests that omit that limit keep their one-hour default. A long episode
  deadline never changes another job's limit or its cache capacity admission.
  The private cache policy's `max_object_bytes` also caps an admitted operation;
  it must cover the complete workspace budget, not only the largest output.
  This consistency check happens before opening source/runtime stores or
  adopting a source. Final disk reservation remains transactional in the runner.
- `reference_staging_bytes` and `reference_picture_processes` bound each native
  scene comparison. `max_objects` and `max_logical_bytes` bound declared resource
  obligations; logical bytes are not allocated disk usage.
- `outputs` maps exact native filenames to byte ceilings: `receipt.json` plus
  `master.mkv` and/or `episode.mp4` as required by the source delivery contract.

The coordinator compiles bounded metadata and retains the complete declared input
set before execution. It does not manufacture eligibility proofs. The worker
restores the same archive, verifies that its planned closure matches the ticket,
then obtains live component owners and fresh primary-reference proofs. Parent
compilation and assembly consume those sealed proofs. Source, byte and runtime
guards stay live across use; root and child identities remain separate to avoid
reacquiring the runner's own fence.

Before compiling child plans or adopting a source, the coordinator verifies the
exact selected component masters through the configured byte provider. A missing
master reports its segment, hash and byte length. This availability check grants
no primary-reference eligibility and copies nothing; the contained worker still
obtains fresh guarded inputs and independent proofs. It never scans or enrolls
an archive tier implicitly.

The runner contains all worker descendants, applies deadline/capacity policies,
retains exact declared results in existing caches and independently reopens its
root result metadata. No primary media are copied into the Git worktree. A job
report does not select a master, approve a candidate, synchronize a delivery or
change project bindings.

Optional `inputs.record_outputs` and `inputs.git_handoff` use the same pinned
template, selector and proposal contracts as native scene jobs. Each output
mapping names a declared media output, an exact archived output-record template
and either its selected slot or an explicitly classified named stream. Input,
scene and component-output definitions cannot serve as parent-output templates.
The engine validates templates before source adoption, emits small candidate
records and an unsigned claim from the sealed assembled result, and retains
those artifacts with the media and receipt. Inherited review status is cleared;
unmapped history stays intact.

The coordinator reopens the retained candidate set and checks exact subjects,
recipe, context, template fields and proposal bytes before owner handoff. The
record contract is pinned in the worker report and completed handoff, so changing
the mapping during recovery refuses. Repeated `finish-episode` also verifies
retained record artifacts. Candidates and proposals remain private artifacts;
the renderer executes no Git commands. Use the retained `git-proposal.json`
with the separately qualified `tig-propose check` and `tig-propose create`
commands, then explicitly adopt the proposal through the project's Git workflow.

After success the coordinator checks the retained worker report, original input
context and live temporary owners. It publishes, finishes, reopens and adopts a
complete context packet under the root job before withdrawing temporary scene,
presentation and parent claims. That packet retains previous result evidence,
selected media, all declared child resources and generated parent metadata.
Budget accounting deduplicates byte identities; contradictory lengths refuse.
The final result reports `temporary_owner_cleanup_required: false`; the immutable
worker report preserves its earlier point-in-time state.

`reel-tig-job finish-episode PRIVATE_PROFILE.json` performs only this metadata
handoff. It can repeat an already completed handoff without journal changes,
resume withdrawals after root adoption, and recover a stop between handoff
prepare and adopt. Recovery rebuilds and reopens the complete context from the
retained result and still-current children. Both the pending selection and the
exact previously committed prepare command must match before adoption. It never
reconstructs a packet from an unchecked pending label alone.

A retained failed result follows the same retain-before-release sequence. It
keeps the original request, failure diagnostics, full declared inputs and every
present component context. A worker may have created no component claims, prepared
some, or adopted some. Prepared components require the exact worker prepare
acknowledgment; they are aborted only after root adoption. Adopted components are
withdrawn afterward. The result and handoff both preserve `succeeded: false`.
The command exits unsuccessfully even when failure cleanup completes safely.

`finish-episode` can complete or repeat that cleanup without rendering; successful
execution of this metadata command does not turn the retained render into a
success. An absent component that subsequently appears, a changed
owner/selection/revision, foreign preparation, missing evidence or budget refusal
leaves remaining obligations live. Interruptions before a complete runner result
is adopted still need a separate recovery route; do not drop those roots manually.

Withdrawing a redundant source claim never removes payload bytes or authorizes
eviction. Cache removal still requires the separate custody and demotion rules.
The command also does not establish generation/text-basis freshness, complete
environment isolation, creative approval or independent backup custody.
