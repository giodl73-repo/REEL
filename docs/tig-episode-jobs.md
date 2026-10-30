# Bounded episode execution

`reel-tig-job run-episode PRIVATE_PROFILE.json` is the explicit episode route over
an already captured source and existing caches, metadata and source-journal
stores. It performs no cache or daemon setup. Keep the machine profile, raw
reports and runtime receipts outside Git.

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

The runner contains all worker descendants, applies deadline/capacity policies,
retains exact declared results in existing caches and independently reopens its
root result metadata. No primary media are copied into the Git worktree. A job
report does not select a master, approve a candidate, synchronize a delivery or
change project bindings. Native output-record/Git proposal integration remains a
separate step.

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
