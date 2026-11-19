# Bounded recipe frontier

`reel.workflow-recipe.v1` pins an authored nonempty base plan and lists future
`TaskTemplate` steps. The combined base/recipe is limited to 128 tasks. Validation
checks unique identities, operation/tool shape, output contracts, declared parent
selectors, source roles and dependency cycles before observing production jobs.
Initial concrete root tasks remain in the base plan; this version does not
create a native job from an empty plan or source context.

`reel_workflow::recipe::inspect` verifies the current plan is an append-only
extension of that base, observes existing jobs through the normal graph adapter,
and computes a bounded wave. New steps become proposals only when every
dependency has an exact verified retained result. Existing steps retain their
waiting, pending, cancelled or refused state. They are not restarted. Their
concrete metadata must still match the recipe template; a mismatch refuses the
step and blocks its descendants, including recipes authored out of topological
order. Existing jobs and candidates remain unchanged.

Eligible proposals preserve authored step order. The explicit 1–16 proposal
limit defers remaining eligible work; it is not a claim that 16 job actors are
available. Every actual admission still checks the configured actor and native
limits. Observations are cached only inside one planning call, with a 32 MiB
observation-evidence bound. They are not atomic custody or first-admission
proofs. The output remains within the existing 1 MiB control bound.

```text
reel-workflow-native schedule-recipe RECIPE PLAN RUN_BINDINGS LIMIT NEW_PRIVATE_SCHEDULE --apply
```

The command uses actual native parent/claim verification and the same guarded,
exclusive, flushed/read-back planning export as `expand-plan`. Each proposed
template pins the current full plan and carries a concrete planning packet and
upstream bindings. The controller can pass that template to `prepare-expanded`;
source freezing, admission, staging, direct/handoff dispatch and retention remain
separate effect boundaries and must revalidate the relevant evidence.

All 60 workflow tests and strict shared Clippy pass. Three protocol fixtures
cover bounded waves and single-session verification, waiting/retained transitions,
descendant invalidation, forward-authored dependencies, authored proposal order,
cycles, missing selectors, limits and immutable base tasks. Native CLI tests cover
metadata export; positive native scheduling qualification remains pending.

Producer-applied recovery/provenance checks preserve original jobs, exact result
fingerprints, explicit budgets and separate approval authority. This is the
recipe decision layer, not automatic actor allocation, effect advancement,
capacity retirement, provider execution or creative/release approval.
