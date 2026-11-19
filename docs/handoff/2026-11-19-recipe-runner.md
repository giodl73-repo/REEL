# Resumable recipe admission runner

```text
reel-workflow-native advance-recipe RECIPE BASE_PLAN BASE_BINDINGS CONFIG PRIVATE_DIRECTORY LIMIT --apply
```

The runner observes the actual native recipe frontier, freezes the next eligible
task and invokes durable expanded-task advancement. It repeats up to the explicit
1–16 admission limit. A later invocation can unlock another wave after a parent
has a verified retained result. Waiting, cancelled and uncertain jobs retain
their original identities. Neither a scheduling proposal nor a timeout means
that an existing operation should be started again.

Configuration schema `reel.workflow-recipe-runner.v1` maps every authored recipe
step to `source`, `admission`, `attempt_id` and `handoff` settings. These are the
existing exact-source, native admission and advancement contracts. Actors are
explicitly enrolled; the runner does not allocate actors or increase Tig limits.
Duplicate index/actor/operation identities are refused before admission. Reusing
an actor requires a distinct operation and a separately completed guarded close.
Actual acceptance still checks slot availability, claim and 128-path bounds.

The owned private root pins recipe, base plan, base bindings, configuration and
directory identity in an immutable intent. It refuses native stores, source and
base-job control namespaces. A scoped runner fence serializes effects. Each
`pending-NNN.json` freezes an original task before admission; its `task-NNN`
capsule owns the admission witness, profile and journal. `state-NNN.json`
checkpoints chain the previous checkpoint and pending-file hashes and preserve
the complete expanded plan/bindings. State zero records the starting graph.

Recovery finishes an original pending adoption before scheduling another task.
It rechecks the configured source, operation, attempt and authored template.
Completed checkpoints are matched to the exact frozen expansion and capsule
bindings. Missing/partial/gapped history is preserved and refused. Lost final
checkpoint response recovery can replay an existing adopted capsule without
another acceptance. No file is truncated or replaced and no fresh attempt is
invented. An interrupted native acceptance remains an explicit recovery boundary.

The graph is bounded to 128 tasks, controls to 1 MiB each, checkpoint history to
32 MiB and the root scan to 512 entries. Returned summaries expose the actual
frontier and checkpoint; an empty advanced list is not a completion or approval
claim. Per-task private profiles and journals remain available for staging,
prompt-batch export, deterministic worker dispatch and original response ingress.
New staging-ticket bindings can be supplied to those commands independently;
the immutable runner checkpoints preserve their original adoption snapshots.

Metadata/claim tests cover bounded sequential admission, original pending
adoption recovery after a lost checkpoint, independent Waiting jobs, blocked
descendants, no-op claims stability, duplicate identities, changed configuration
and preservation of partial checkpoints. A separate existing-pool fixture is
being qualified for two staged tasks, retained synthetic parent completion and
automatic admission of the newly eligible dependent.

Provider execution, scratch allocation/retirement, actor close, candidate
selection and creative/release authority remain separate. Producer-applied
recovery, custody and provenance checklists do not constitute independent review.
