# Native effective-picture editor review

Scope: source-reader integration, sorter intake/apply, native graph-lock API,
synthetic fixtures and contracts. Base editor `9ba3b08`, reader `2da4465`.
Producer-applied Editor, Animation Director and Rights/Provenance checklists;
no human creative approval, independent review or runtime adoption is claimed.

| Finding | Correction and evidence |
| --- | --- |
| Shared defaults and local picture overrides could disagree across preview/draft/graph | Rows now use native materialization from the shared reader. No-op preserves defaults and overrides; substitution remains in the original layer and rebinds both graph event and selected slot. Positive and malformed-override cases pass. |
| Schema-name-only intake accepted skeletal native controls | Native scene/bindings, measured alignment identity/bounds, exact alignment-binding bytes and graph/pointer validation now run before media reads or draft creation. The synthetic fixture uses valid native scene, bindings, alignment and graph data. Unknown native fields reject; accepted open content and other copied controls survive. |
| Manual graph/revision transformations diverged from native operations | Picture changes append a native selected revision with supersedes ancestry; timing-only edits preserve slot revisions. Locks reuse assembly's canonical routine. The prior checkpoint already hashed its full graph body; this unifies algorithms rather than claiming it omitted timing. New core lock tests cover stable neutralization, requested identity, graph changes and invalid inputs. |
| Multiple events could request different assets for one selected slot | Conflicting shared-slot edits reject before output creation. Current shared references remain consistent; the source is untouched. |
| Successful synthetic tests alone do not establish private production compatibility | Three existing private requests pass metadata-only application: identity, reorder/holds and removal (40/40/39 events). Every original control hash stays unchanged and draft pointers match their graphs. This does not rerun the full private compiler, render, measure audio, review motion or grant acceptance. |

Validation: all 272 Rust tests across reel-assets and reel-assembly pass,
including 15 sorter cases, six source-reader cases and two new core-lock cases.
Six browser-logic tests, strict all-target Clippy, formatting, ROLES and diff
checks pass. Private controls/results stay in machine-local scratch outside
the repository; public
tests contain only synthetic data. No live cache reads/writes, media exports,
render, shared runtime replacement, selected-version change or eviction occurs.

Remaining gates: explicit runtime adoption; full private compiler recheck for
this stricter intake; multi-cue/bilingual editing; configurable project-path
intake; hostile/reparse-filesystem custody; and explicit handling of an incomplete
new draft/export directory after failure. The tool creates candidate drafts,
not publication or deletion authority.
