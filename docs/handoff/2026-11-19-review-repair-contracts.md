# Shared review findings and repair impact

`reel-workflow::review` now supplies private review contracts for film and comic
editions. A review copy pins project, exact source commit, language, publication,
review derivative and existing producer unit IDs. Film targets carry half-open
frame ranges and optional cue IDs; comic targets carry ordered page, panel and
balloon IDs. These are declared mappings: the checker does not allocate IDs or
prove that a supplied mapping was exported by a renderer.

Findings name a unit, defect code, requested change, declared review modality,
evidence pins and project/REEL/Tig route. The finding key includes the exact
review-copy fingerprint, unit/range and defect code. It stays stable across prose
and evidence updates, but changes across languages, copies or target versions.
Identical repeated findings coalesce; contradictory duplicates refuse. A new
description does not create a fresh issue operation for the same finding/route.

`check_returned_report` connects this contract to the existing ReviewFinding
task/result protocol. It checks actual copy/report bytes against the task/result
pins, matching project/commit/language, declared publication/review inputs and
every finding evidence pin against declared inputs or returned artifacts. This
works with either provider or manual responses; it does not establish viewing,
provider provenance, native byte admission or retained ownership.

The local CLI exposes:

```text
reel-workflow check-review COPY REPORT
reel-workflow check-review-result PACKET RESULT COPY_INPUT COPY_PATH REPORT_OUTPUT REPORT_PATH
reel-workflow export-review-issues COPY REPORT ROUTES NEW_PROPOSALS
reel-workflow plan-repair PLAN COPY REPORT REPAIR NEW_IMPACT
```

Issue proposals use configured `owner/repository` routes, normalized to lowercase,
and stable operation keys. Outputs contain private review context and are created
exclusively; no tracker request is sent. A future authorized adapter must preserve
remote issue URLs, reconcile uncertain creation under the original operation and
prepare synthetic reproductions for generic REEL/Tig defects before sharing.
These local proposals are not sanitized public issue bodies.

A repair pins the checked report, selected finding keys and exact before/after
artifact descriptors. Its impact is computed from exact task input pins and the
declared dependency DAG, preserving topological rebuild order. A Spanish text edit
can affect Spanish lettering, its reading proof and a combined issue without
changing English lettering. A shared art edit affects both languages. Required
contextual languages include all affected tasks, and the original full-edition
review descriptor identifies the context that must be checked again using newly
pinned corrected media. A small rebuild does not waive neighboring-page or
full-film review.

`unaffected` means no data/dependency path from these proposed edits in this
supplied plan. It is not permission to reuse old jobs after source-commit, tool,
configuration or other task-key changes: the normal native frontier/admission
checks still apply. This command does not rewrite plans, apply edits/selections,
retire work, release claims, execute repairs or close issues.

Controls and exported proposals/impact are bounded to 1 MiB. Additional bounds
include 128 findings/edits, 16 evidence pins per finding and 4,096 declared units.
Empty reports are valid; empty repairs are refused. Existing task/result schemas
and legacy task keys are unchanged.

Nine new tests cover both formats, language/version isolation, frame boundaries,
comic order/modality, duplicate findings, stale repair baselines, exact returned
bytes/evidence, issue routing/operation keys, output bounds and all four CLI
commands, including preservation of existing exports. Test logs and qualification
pins are recorded under
`F:/src/tig-dev-temp/reel-review-repair-contracts-20261119-v1`.

Producer-applied editor, comics editor and rights/provenance checklists: full
context remains a review requirement; declared modality is not proof of watching
or listening. Canon/translation, performance/voice, fonts, selection and release
decisions remain with actual project authorities. No independent review is
claimed. Root producer-map export/binding, native structured-review ingestion,
authorized tracker effects, scoped repair execution and evidenced closure remain
open, alongside the other completion-ledger requirements.
