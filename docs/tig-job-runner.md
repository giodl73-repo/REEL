# Tig-backed bounded jobs

The optional `tig-publication` feature exposes `reel::tig_job_runner::run`.
It connects an already adopted planned Tig job context to the existing
`reel-assets` runner. It does not create another runner, cache or inventory.

Provide the exact read-back `JobContext`, authoritative `SourceJournal`, owner,
source revision, operation key, existing metadata directory and trusted minimum
checkpoint. The explicit cache `Config` and shared coordination directory must
use the same enrolled roots as the other project workers. Names and drive letters
are project configuration; an episode cache on F and music cache on E are examples.

```text
planned job metadata -> finish/reopen/readback -> owner prepare/adopt
                                                  |
                                     one source-scoped job fence
                                                  |
job request -> exact role/size checks -> capacity reservation
                                                  |
                          pinned inputs + bounded owned workspace
                                                  |
                       existing Windows process-tree containment
                                                  |
                     verify outputs / salvage declared failure outputs
                                                  |
                       existing CAS publication + bounded evidence
                                                  |
                 result tree -> finish/reopen/readback -> owner adopt
                                                  |
                   fresh retained-byte checks -> retire owned scratch
```

Inputs must belong to the admitted Primary, Control or Interpretation roles;
tools must belong to Runtime. Exact hashes and sizes are required before source
lookup or capacity admission. The runner still verifies actual local bytes and
keeps read protection during execution. This trusted-command runner is not a
sandbox for hostile tools. Installed-tool pins do not discover neighboring DLLs
or the complete environment.

Retention links the original job context, exact request, compressed evidence and
each verified output. It finishes metadata, reopens and checks it, then uses the
same journal authority, owner, source revision and current selection for owned
prepare/adopt. The input root remains a predecessor obligation. Arbitrary command
results remain unproven: technical success does not create a native production
claim, creative approval or delivery status.

Execution failure can retain bounded declared candidates and diagnostics. A
retention failure preserves the workspace and does not turn a successful producer
into a failed producer or invoke a contradictory second retention attempt.
Ambiguous journal transitions keep their existing requirements; recovery needs
the exact owner state. `retain_workspace: true` remains an explicit diagnostics
exception: it skips this retention/retirement handoff and keeps scratch.

`reel_assets::jobs::run_admitted` exposes the reusable ports independently of Tig:
`RunContext` supplies `Config`, a role-sensitive `ResourceAuthority`, and a
`RetentionSink`. The sink receives immutable runner-created descriptors, not
deserialized success JSON. Returning success promises durable owned retention;
the runner then independently checks canonical bytes and exact workspace scope.
Legacy `run` and checkpoint-resume paths keep their registry adapters.

The current bounded runner executes on Windows. Existing scratch reservations
are conservative and stay in force throughout work; this does not integrate
Tig's complete physical-allocation ledger. Metadata availability and one admitted
authoritative journal remain caller obligations. Native managed CLI execution,
all-worktree claim coverage, interrupted handoff recovery commands, independent
archive custody and retained-cache eviction remain separate work. No source or
cache payload is removed by this integration.

## Native coordinator and worker

`cargo run --locked --features tig-publication --bin reel-tig-job -- run profile.json`
runs an explicitly configured frozen native scene through the bounded runner.
It requires Windows, existing metadata/journal/control/CAS directories and trusted
minimum checkpoints. It does not install a runtime or initialize those roots.

```text
private profile + frozen context + explicit tool bindings
                       |
       coordinator binds its actual executable and programs
                       |
          restore -> recipe -> validate bounded job request
                       |
        freeze/readback JobPlan -> owner prepare/adopt
                       |
        pinned same-binary child in bounded owned scratch
                       |
       child rebinds actual host + FFmpeg + FFprobe + dependencies
                       |
       restore exact context/recipe -> compile/render/check
                       |
       exact complete declared output set + receipt + worker record
                       |
          Tig retention handoff -> retire disposable scratch
```

The private profile schema is `reel.tig-native-job-profile.experimental-1`:

| Field | Meaning |
| --- | --- |
| `worker.metadata_root`, `worker.metadata_minimum` | Existing metadata provider and trusted minimum checkpoint |
| `worker.source` | Admitted `reel.tig-archive-input-profile.experimental-1`; Git is not needed during execution |
| `worker.context` | Exact frozen native context Selection, not a latest lookup |
| `worker.request` | Native scene Request matching the frozen descriptor |
| `worker.runtime` | Private Bindings: actual CLI host byte identity, explicit FFmpeg/FFprobe FileRefs, named dependency FileRefs |
| `worker.outputs` | Explicit complete native output filenames plus `receipt.json`, each mapped to a positive maximum; omit when using `output_budget` |
| `worker.output_budget` | Alternative `{ "default_max_bytes": ..., "overrides": { ... } }` policy; exact override names must exist in the derived contract |
| `cache` | Existing reel-assets Config, including an existing absolute shared `coordination_root` |
| `journal_root`, `journal_minimum` | Existing authoritative owner journal and trusted minimum checkpoint |
| `source_id`, `owner`, `expected_revision` | Exact caller-owned job identity and current revision; zero is required for a new source |
| `start_operation`, `retain_operation` | Explicit distinct-purpose stable operation identities |
| `budget_bytes`, `scratch_bytes`, `timeout_seconds` | Complete workspace/publication allowance, scratch allowance and bounded step deadline |

The profile is limited to 128 KiB. The inline worker admission must fit 64 KiB;
there are at most 511 native outputs. Native outputs are retained under
`native/<filename>` roles, with a separate bounded `worker.json` completion record.
All native receipt outputs must be declared; extra or omitted names cannot report
successful retention. Budgets must include two copies of declared output maxima
for atomic publication, control bytes and scratch. Installed runtimes execute in
place and primary inputs resolve directly from the existing cache enrollment.
The worker's compiled controls and intermediates occupy its bounded scratch.

The coordinator keeps its verified runtime guards while the child runs. Both
processes derive the producer from the same declared runtime manifest and exact
native recipe. The coordinator pins the private ticket hash/size in process
arguments; a changed ticket refuses before parsing or tool/metadata lookup.
The child receives no journal handle or write authority for owner
transitions. A hidden `worker` subcommand is the trusted internal child entry;
invoking it directly does not provide admission, containment or retention.

Success reports the retained outputs and resulting owner-journal checkpoint.
It does not update project `.tig` files, approve a master, publish externally or
export a reusable native production claim. The generic retained result deliberately
reports `production_claim: null` and incomplete claim coverage. Metadata validity,
actual renderer checks, owned byte retention and creative selection stay distinct.
Errors do not reset source state: interrupted starts/handoffs require exact owner
recovery, and retention uncertainty preserves scratch. These profiles and receipts
contain private local information and are not shareable public diagnostics.

## Deriving outputs before rendering

Supply `worker.output_budget` instead of `worker.outputs` to derive the complete
native output list. For example:

```json
"output_budget": {
  "default_max_bytes": 268435456,
  "overrides": { "receipt.json": 65536 }
}
```

`reel-tig-job plan-outputs profile.json` runs a **retained bounded planning job**.
It verifies inputs and may invoke the explicit media probe; it does not encode a
scene. It publishes only `planning.json` plus the runner's request/diagnostics and
adopts the result root. This is an owned job with capacity/time/scratch admission
and durable metadata transitions, rather than a read-only or dry-run command.

`reel-tig-job run profile.json` with that policy first runs planning, verifies the
exact retained report bytes and current owner/revision/root, then derives the
render request. The actual worker rechecks the full native output contract before
encoding. Existing explicit output-map profiles still work and now reject an
incomplete output set before spending the render.

```text
new source@0
    -> planned native inputs@2
    -> retained planning result@4      (input predecessor retained)
    -> same planned native inputs@6   (planning predecessor retained)
    -> retained render result@8        (all three predecessors retained)
```

These numbers are source-journal revisions, not Git commits or asset versions.
Each phase uses its own scoped operation keys and the existing per-source fence;
foreign, stale, pending or aborted result state refuses. A planning report is read
only after the owner check and must match its CAS pin, exact retained tree/header,
source context, recipe, producer and budget policy. Both phases keep their earlier
requirements; dropping them or reclaiming their bytes needs separate lifecycle
authority. Use fresh attempt operation identities for a new request, not a reset.

The renderer and checker share `scene_delivery::output_contract` for base video/
audio, overlays, fonts, post-compose camera files and Motioncraft evidence. Its
`raw_pcm24_stereo_bytes` and `raw_rgb24_video_bytes` describe one uncompressed
stream's data, excluding container/encoding overhead. They are reference sizes,
not predicted compressed sizes or physical allocation. The report's
`atomic_native_output_bytes` is twice the sum of declared native output maxima;
the final job also needs scratch, controls and the worker completion record.
Overflow and unknown/zero overrides refuse. Planning reserves the configured
scratch allowance capped at 64 MiB for metadata-only work, plus its bounded ticket,
a 256 KiB request-control allowance and
two 64 KiB report copies. It must fit the profile's workspace ceiling, but does not
reserve the full render allowance. The render phase keeps the configured full
budget and scratch allowance. Exceeding the planning control cap fails the bounded
job; it does not borrow the render allowance. These reservations still require the
cache's normal free-space margins.

Native requests explicitly set `output_count_limit: 512` in the reusable runner;
other requests keep the default64 unless they explicitly select a bound from1
through512. Intake, per-file/path, diagnostics/evidence, tree and total workspace
bounds remain in force. Large evidence sets are refused explicitly when they
exceed these bounds; they are never silently truncated into a successful result.
