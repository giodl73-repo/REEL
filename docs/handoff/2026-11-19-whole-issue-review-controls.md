# Producer-derived whole-issue review-task controls

The native `ComicReviewControl` producer now accepts `ComicIssueAssembly` as
well as `ComicRender`. Both routes bind the exact original worker receipt,
publication, language/edition, dependency result and complete declared input set.
The issue route binds its original assembly contract and receipt, replays every
captured part, checks the replayed assembly receipt, and derives all ordered
page/panel/balloon targets and reading-image pins before writing controls.

The existing three JSON outputs and receipt schemas are unchanged. The generated
copy names the whole issue's source commit; the generated structured-review
request references that exact copy. The existing 128-input, 64 MiB comic input,
128 MiB captured issue and 4,096 review-unit bounds remain explicit. This adds no
film limit and grants no print, creative or release authority.

The renderer-free portable library provides `generate_with_issue` for a caller
that can supply bounded issue capture/replay. Its existing `generate` entry keeps
page behavior and refuses whole issues without an explicit capture. Captured
metadata alone does not prove native custody or deterministic rendering. The
native adapter supplies its full deterministic replay callback under the child's
original native claims/frozen parent context; retained-parent admission already
checks the selected parent's original producer lineage.

The new portable contract test checks complete controls, exact parent/edition
correspondence before capture, capture-required refusal and missing page refusal.
Its placeholder media establishes metadata correspondence only. An ignored
native qualifier borrows the existing retained two-page synthetic issue, holds
its original retained fences, reads exact native snapshot inputs and retained
outputs, and invokes the actual native replay callback. It checks six targets,
both reading pins, structured request binding, changed original receipt refusal
and an unchanged actual Index checkpoint. The frozen qualifier passed in 105.70
seconds with exit 0 on 2026-11-20. Both reading pins, all six targets, the
structured request and changed-original-receipt refusal passed. All 133 workflow
and 85 standard native tests pass (74 explicit native fixtures excluded), with
all 247 original SDK pins and strict all-target Clippy for workflow and native
with/without film.

Evidence is retained at
`F:/src/tig-dev-temp/reel-whole-issue-review-controls-20261119-v1`.
`qualified-controls.json` SHA256 is
`213dfbe6afc0de721dbd60bc2444081075b87411d4c5b0c5c20a624a041bddbc`.
It pins the controls, prospective child/report packets, result, terminal log and
source/tool packet. Source commit
`d393ae49b89dfd208adb7ff49601c3471c2221fc` has 1,961 verified archived files,
source-manifest SHA256
`9931fb12b2e4e605a8cceb28e4e620b4273ef7804c9dce382b0e8ad732526dab`.
The frozen fixture is 29,321,728 bytes with SHA256
`bb791052038ff378b9349bed00a108f6ca0e086133602eac5958cf7ab71cc9cd`.
The candidate native CLI is separately frozen at 39,210,496 bytes with SHA256
`e0ce54d8c1aabdda41a72434aaca10f91336766c4139dee89e7ab8b2c08b81d4`;
it is only pinned in prospective controls, not adopted or invoked for lifecycle
effects. The original B9 producer runtime stays unchanged. Regression log pins
and strict-check results are recorded in `qualified-validation.json`.

This earlier qualifier generates prospective task/control bytes without admitting a
child or selecting a slot. Owned child input protection, retained control
completion/recovery and the subsequent complete issue model/review/fix lifecycle
remain open. Complete-issue parents can themselves have upstream inputs: the
child's explicit fixed/upstream declarations must account for those original
part/control producers; copying the assembly's source profile is insufficient.
Existing profiles/runtimes and the protected Waiting control attempt remain
unchanged. No provider/tracker call, real S4/S5 adoption, scratch retirement or
payload deletion is part of this qualification.

The subsequent [owned whole-issue control qualification](2026-11-19-owned-issue-review.md)
now carries those exact ancestor selectors, closes/reuses a healthy original part
slot and reaches native retained control completion/replay. Protected structured
review and the complete issue model/review/fix lifecycle remain separate.

Producer checklist findings follow `.roles/ROLE.md`, `comics-editor.md`, editing
and rights/provenance: whole-issue order and exact targets are preserved; actual
page-turn/rhythm, lettering readability, translation/canon/font authority and
human viewing remain separate. These are producer checks, not independent review.
