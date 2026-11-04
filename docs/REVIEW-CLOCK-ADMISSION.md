# Frozen review resumption (in development)

`tig_review` supplies metadata predicates for a frozen, asset-backed image review.
It does not render, capture, probe, view images, approve creative work, publish
assets or grant deletion authority. Production qualification remains separate.

The observation policy keeps project scope, footer field names and target checks
in declarative data. Target rules name the source span, exact fix text, original
and corrected picture fields, and required nullable appearance facts. The engine
requires exact sample IDs, frames, roles, fresh per-role observations, preserved
holds and explicit false human approval. A qualified finite scope cannot contain
a major obstruction; required footer facts must be true on relevant clocks.
False and null outcomes remain distinct. Source reasons and footer events come
from the frozen task, not from reviewer-authored replacements.

The byte-verification callback is mandatory for every role and raw reference.
It must reopen the exact object through the consumer's protected existing-byte
source. Self-reported fresh-byte flags cannot replace that operation. PNG checks
validate the declared dimensions and RGB8 native-role format after byte checking.
Retained proof checks bound gzip expansion and compare the exact request bytes,
result, output tables, caps, sample/frame mappings and requested view pins. They
never execute the retained request or read historical checkout/workspace paths.
The proof verifier accounts for the exact native retirement transition: the gzip
contains the pre-retirement result, while the returned result may add that gzip's
identity and change only `workspace_retained` to false. Other differences refuse.
External review JSON supports fractional timestamps with duplicate-key rejection;
original byte hashes remain authoritative. Tig canonical identity JSON is unchanged.

Subreports can be reconstructed and revalidated as completed skips. Aggregation
requires disjoint exact task clocks from one original group and restores source
order. Partial coverage stays explicit, holds and raw observations are preserved,
and group coverage never promotes whole-programme qualification.

Scope reservation uses an existing explicit control directory, keyed by immutable
task identity. Method and policy pins are in an immutable receipt. A second claim
refuses even if its policy or owner changes. A pending receipt never automatically
permits more views. Reports and completion records use exclusive, flushed,
read-back writes; an interrupted completion can validate the saved report without
new views. Existing outcomes cannot be overwritten. Receipt files are metadata
coordination only, not asset publication or independent custody proof.

## Explicit existing-byte CLI

`reel-tig-review` requires `tig-publication`. Every operation takes an absolute
local profile, exact retained task/method/policy pins and an existing control
root. Its byte provider uses the declared existing places and protected read
budgets; no cache setup, discovery, copy or deletion follows entry.

- `inspect PROFILE`: verify source feature notes, group selection, source pins,
  native request/result/root/gzip proofs and all PNG headers; reserve nothing.
- `reserve PROFILE --owner OWNER`: perform those checks before creating one
  immutable receipt. A counted profile validates and skips without a new receipt.
- `resume PROFILE`: require the exact receipt and its method/policy binding.
  Pending claims do not automatically permit more views. Saved reports validate
  and skip; an orphan report can complete without repeated observations.
- `submit PROFILE`: require a retained evidence object and the receipt. Validate
  all observations and save once. A completed retry validates and skips before
  processing new evidence. Use the existing publisher to retain the saved report.
- `aggregate PROFILE OUTPUT`: reopen exact retained parts with one shared read
  budget, revalidate their native proofs and observations, enforce disjoint frozen
  group coverage and preserve source order. OUTPUT must be an explicit file in
  the existing control root. A changed aggregate cannot overwrite an old one.

Scope profiles explicitly declare `state.kind` as `unseen` or `counted`. A counted
state names the exact retained report and never schedules a view. Recovery and
submission also bind the exact receipt; submission names the exact retained
evidence. Policy data declares schema names, method identity, role dimensions,
native media input mappings and target rules. Target lists are rederived from
the pinned feature notes, including configured exact-text fallback pointers;
dropping a target or substituting raw sources refuses before reservation.

All CLI summaries keep human approval false. A saved local report or aggregate
remains publication-pending until the existing asset publisher retains it.

## Explicit reinspection of a held scope

A counted scope still skips in the ordinary commands. A separate
`reel.review-reinspection-profile.experimental-1` profile can request additional
observations of the exact unchanged task when its retained prior subreport is
finite-false. It contains `scope` (the ordinary counted profile), a bounded
`reason`, and a nonempty `prerequisites` map. Each prerequisite has `origin`
(`task` or `feature-notes`), a JSON `pointer` to its original object descriptor,
and the matching `object` pin. Undeclared/substituted or corrupt objects refuse.
Producers choose the required context; byte verification does not establish its
meaning, complete historical protocol parity or actual human reading.

Use `reinspect-inspect`, `reinspect-reserve --owner OWNER`, `reinspect-resume`
and `reinspect-submit` with that explicit profile. All original task, native
proof and image-header checks and all prerequisite reads precede reservation.
Only reservation creates metadata under the existing control root's
`reinspections/PRIOR_REPORT_SHA256` namespace. The immutable context binds the
original task/report, policy/method, reason, prerequisite declarations and prior
holds. Changing these cannot create another claim for that report. Qualified,
unseen or already-reinspected prior reports refuse.

The returned `context` pin must appear as `reinspection_context` in the newly
retained fresh evidence. The new report preserves the complete context and prior
holds under `reinspection`; the original report is never overwritten. Every
reinspection summary says `new_planned_clocks: 0`. Actual additional view counts
remain in the new report. Reinspection adds no previously unseen planned clocks,
clears no old finding automatically, and grants no global or human approval.
Pending receipts still grant no repeat-view permission. Completion recovery and
submission retries validate the saved report without new views.

This path requires its own complete hosted qualification before real image
observations or production adoption. The earlier ordinary-admission binary and
bindings are not upgraded by a source merge.

## Remaining qualification before adoption

- Demonstrate full predicate parity on real frozen corrected/retained target
  scopes and previously counted reports. Preserve every original negative/null
  outcome and hold. Never trust retained verification booleans or old paths.
- Qualify the complete CLI, interrupted saves and corrupt proofs through hosted
  Linux/Windows checks; the earlier library-only checkpoint is insufficient.
- Only then admit a real remaining scope. Never substitute these library tests
  for fresh private observations or promote old false/null/hold outcomes.

No active production runtime pin changes with this development checkpoint.
