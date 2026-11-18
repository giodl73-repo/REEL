# Explicit workflow comic parts

Portable `comic_issue_source::{Plan, Part}` contracts now live outside the
optional renderer feature. The existing `comic_issue` API reexports them;
`comic-issue-check` works in the metadata-only executable and reads no media.
The plan's JSON schema and grouping/spread rules remain unchanged. Checked
projections preserve original page IDs, source descriptors and reading direction.

`ToolContract` has an optional `part` selector with schema
`reel.workflow-comic-part.v1`, an exact plan Artifact and a part ID. Absent fields
stay omitted from serialization. The selector must point to a distinct declared,
bounded JSON input, not a producer-relative file alias. Binding verifies the full
master publication, plan and edition, then the selected group's exact logical
page/font/art/request closure. Part output contracts belong to the task; final
edition contracts remain with the master. Wrong master, language, part or logical
source refuses; a partial request without a selector cannot masquerade as a full
edition. Master/plan changes invalidate task keys; narrower cross-commit reuse
still requires an explicit checked equivalence contract.

Shared fixtures check selected-only reads, exact master/plan correspondence,
undeclared/aliased plans, stale results and old absent-field serialization.
Portable metadata/CLI fixtures cover spread preservation and validation without
fonts or a renderer. All 55 shared tests and the ten metadata-only regression
cases pass; strict Clippy passes in both dependency graphs.

The new existing-store fixture declares a two-page master and two authored parts,
but admits only the opening part. The other page's record exists in the committed
history; its byte payload is neither declared as a job input nor published by the
fixture. The selected master/plan are protected named inputs. The fixture exercises
native candidate/retained completion, bare-history resume, packet export,
source restoration, exact rebuilding and a lettering edit, then checks the unused
page has no protection claim in this job's controller. All 35 default native tests
and seven explicitly invoked existing-store fixtures pass, including both the
original whole-edition path and the selected-part case (97 distinct tests together
with the shared suite). Metadata-only cases are repeated in the separate default
feature build, not counted twice. Formatting/diff checks and strict Clippy pass.
Hosted CI has not run. Synthetic packets, frozen tools and exact source manifests
remain outside Git; no project assets are rendered or adopted.

This is selected-part admission, not a complete native issue scheduler. It does
not admit the closing part, assemble/retain a final native issue, release actors
or select candidates. Native input/output/128-path/16-actor bounds still apply;
metadata's 32-page group ceiling does not guarantee actual job admission. Full
issue assembly needs explicit retained upstream task dependencies and a producer
adapter using the same frozen renderer graph as its parts. Root/native lockfiles
can differ; matching bytes cannot be inferred from a similar command name.

Project adoption, contextual issue/fix review, creative/translation/font/print
permissions and release remain open in the [completion ledger](../workflow-completion.md).
No project profiles/assets, existing producer SDK or selected Git records change.
