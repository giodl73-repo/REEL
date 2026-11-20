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
and an unchanged actual Index checkpoint. Qualification is pending.

The qualifier generates prospective task/control bytes without admitting a
child or selecting a slot. Owned child input protection, retained control
completion/recovery and the subsequent complete issue model/review/fix lifecycle
remain open. Complete-issue parents can themselves have upstream inputs: the
child's explicit fixed/upstream declarations must account for those original
part/control producers; copying the assembly's source profile is insufficient.
Existing profiles/runtimes and the protected Waiting control attempt remain
unchanged. No provider/tracker call, real S4/S5 adoption, scratch retirement or
payload deletion is part of this qualification.

Producer checklist findings follow `.roles/ROLE.md`, `comics-editor.md`, editing
and rights/provenance: whole-issue order and exact targets are preserved; actual
page-turn/rhythm, lettering readability, translation/canon/font authority and
human viewing remain separate. These are producer checks, not independent review.
