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
