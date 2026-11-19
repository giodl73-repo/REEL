# Protected structured review reports

The explicit `review-report` intelligence operation adds semantic validation to
the existing protected handoff/provider lifecycle. Legacy `review-finding` tasks
keep their free-text behavior. Existing task/result encodings and legacy keys
are unchanged.

The new operation requires the exact JSON input `control/review-request`:

```json
{
  "schema": "reel.workflow-review-request.v1",
  "production_commit": "<exact reviewed production Git commit>",
  "copy_input": "control/review-copy",
  "report_output": "report"
}
```

The copy input and named required JSON output are bounded to 1 MiB. Request,
copy, reviewed publication/media and evidence pins enter the ordinary declared
input closure. The task's source commit is the consumer admission snapshot;
`production_commit` separately pins the reviewed version. These may differ when
new controls are admitted for an older film/comic. The pure `review::Bound` owns
the exact checked request/copy and validates the returned report. The legacy
copy-only checker refuses this operation so it cannot bypass request binding.

Metadata admission checks the request descriptor. Guarded preparation reads and
checks the request/copy, project/language/production version and required output
contract before manual export or provider execution. Missing/invalid controls
leave admitted protection intact. Admission alone does not mean bytes are ready.

External response registration/ingress checks the structured report before
recording candidate work or publishing returned bytes. Invalid units, stale copy
pins, malformed reports and undeclared evidence refuse with structured errors.
The native handoff preflights these bytes before recording ImportPending, so a
semantic refusal leaves Waiting and its original journal record unchanged.
A refusal before registration has no candidate effect; uncertain effects still
require reconciliation under their original operation, not a fresh attempt.

Native retention preflight/readback and historical verification independently
reopen declared inputs and every returned artifact, then recheck report semantics
under retained coverage. The resulting guard exposes `review_report()` only for
a verified structured review. Source/output gates reuse already-held input
objects, and generic output opening is skipped for this operation to avoid
double-opening non-reentrant native gates. Those guards drop before new managed
retention effects, preserving the qualified lifecycle order.

Input and output sets are each bounded to 128 MiB, with existing source reader,
actor and changed-path limits still applicable. No new cache engine or implicit
selection, claim release, cleanup or issue delivery is introduced.

Qualification is pending. The existing-pool fixture consumes the prior native
comic review export and actual reading-image pins, admits new controls, refuses
stale findings with unchanged claims, then ingests/retains a valid report under
the original attempt, completes the handoff journal and replays without caller
return files before verifying using bare Git metadata and native bytes. The
embedding Rust executable is itself the pinned lifecycle runtime in this fixture.

This validates declared maps and native custody; it does not authenticate that
arbitrary caller-authored maps came from a producer. The separate native review
export supplies producer-derived maps, and a future derived-control/upstream
adapter must connect that lineage automatically. The fixture performs no provider
call or creative review and leaves its source job active. Actual service execution,
post-close history for this operation, issue effects, scoped repairs and evidenced
closure remain separate qualification work.

Producer-applied editor, comics editor and rights/provenance checklists preserve
version/language scope, exact source wording and full contextual review. Native
report retention grants no creative, translation, performance, font, selection,
print or release approval. No independent review is claimed.
