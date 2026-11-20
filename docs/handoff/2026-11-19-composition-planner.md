# Complete authored composition closures

`reel-workflow::composition` checks a complete authored resource declaration
against an existing bounded recipe. It supports film and comic-issue planning,
explicit per-language final tasks, ordered components and separately identified
control parents. It exports the exact checked recipe for the existing scheduler.
It does not choose a partition automatically or raise native lifecycle limits.

```text
reel-workflow check-composition DECLARATION
reel-workflow export-composition DECLARATION NEW_RECIPE NEW_REPORT
reel-workflow-native schedule-recipe RECIPE BASE_PLAN RUN_BINDINGS LIMIT NEW_SCHEDULE --apply
```

The first two commands read only their named declaration and write only new
private controls. They never open media, resolve a native object, admit a job,
invoke a provider, render or change selection. Export validates first; existing
destinations refuse, and a partial two-file export is preserved for diagnosis.

## Declaration and accounting

`reel.workflow-composition.v1` contains `format` (`film` or `comic-issue`),
`recipe`, `resources`, `editions` and `limits`. Resource binding names point to
an exact artifact descriptor and its **consumer task context commit**. This
context is not an assertion of Git/annex origin, stream choice, retention or
availability. Native declarations and original retained-parent proofs remain
mandatory. Different scoped task versions can consume the same logical ID with
different exact descriptors; byte equality alone does not merge those bindings.

The resource registry must exactly cover all fixed descriptors on the reachable
composition graph: ordinary task inputs, tool publication/request/source/part
roles, executable/auxiliary pins and existing native/upstream control descriptors.
Unused bindings, duplicate exact bindings, missing or stale contexts and a hash
with conflicting byte lengths refuse. Shared bytes retain distinct logical
bindings. Reports distinguish summed logical source bytes from distinct object
count/bytes; this is accounting, not another storage engine or cache census.

Each edition names a final task with its language and an ordered `components`
list. The final task's directly consumed parent task IDs must equal components
plus optional unordered `controls`, with no duplicates or overlap. A dependency
alone does not count as a consumed component. Component/control languages must
match or be explicitly shared (`None`). All new recipe stages must reach at
least one final edition. Existing unrelated base tasks are preserved but are
outside the selected composition's resource scope.

Per-stage input counts and worst-case bytes include deduplicated fixed inputs
and distinct parent output selectors. Parent output-contract maxima and the
1 MiB worker-receipt ceiling are used before actual results exist. Legacy
upstream's 64 MiB allowance or explicit streaming total/object allowances must
cover those ceilings. The declaration additionally bounds source bytes (at most
128 GiB), stage inputs (at most 64 GiB), objects (at most 16 GiB and input total)
and inputs per task (at most 128). Recipe/task limits and the 1 MiB control bound
remain unchanged. Native previews must still count their complete extra source,
runtime, envelope and retained resources against actual actor/128-path limits.

## What the report proves

The report pins the exact declaration and recipe hashes, ordered edition intent,
resource totals and each reached stage's fixed bindings, context and input
ceilings. It explicitly reports native binding, production admission, producer
order/closure and task completion **false**. In particular, an opaque job/control
payload can contain additional renderer dependencies which this metadata checker
does not interpret. A native producer must independently prove its complete
reference closure and enforce component order from its actual request/conform
receipts. Reordering declaration components changes the declaration hash; it
does not silently rewrite producer controls or claim a changed rendered film.

## Tests and remaining gates

Synthetic fixtures declare 600 resources / 7,936,104,893 logical bytes, including
3.36 GB and 2.9 GB descriptors, over seven explicit components. Their task
templates are synthetic proposals, not a claimed E7 producer conversion or an
inspection of the real component partition. Other tests cover hidden control
parents, tool/runtime pins, scoped drawing versions, source aliases, disconnected
steps, missing/extra/stale resources, absent upstream budgets, bilingual shared
comic art and explicit per-edition order. CLI checks/export/no-clobber exercise
only metadata and produce a schedulable recipe.

Record final test/tool pins and the retained synthetic example before claiming
qualification. The pending 3.6 GB native lifecycle is not retried by this work.
Automatic episode partitioning, actual native closure protection, producer
composition, bilingual conformance/captions/credits/delivery and project-owned
adoption/rollback remain open. Producer-applied editor, comics editor and
rights/provenance checklists keep declared order separate from actual rendered
reading/cut order, exact approvals, source canon, voice/likeness, editability,
printer proof and release. No independent or human creative approval is claimed.
