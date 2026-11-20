# Whole-issue producer-derived review targets

`export-review-copy` now accepts retained `ComicIssueAssembly` results through
the complete native issue replay proof. It derives the whole edition's existing
page/panel/balloon IDs and ordered reading-image pins from the captured part
requests, selected page sources and exact part receipts. The existing comic
review-copy/manifest schemas remain unchanged. Native guards stay live while
writing the new private controls.

The portable helper checks the original task and assembly receipt, all referenced
part/request/page/reading byte pins, selected page/panel/utterance order, language,
edition and reading logical identity. The complete captured map remains bounded
by the existing issue output budget, and review's existing 4,096-unit limit
remains enforced. Matching bytes alone do not establish native custody: the
native route first checks retained ownership and deterministically replays the
entire issue, including the v2 original-producer correspondence checks.

A contract test covers page/panel/balloon export and refuses changed reading
bytes, missing selected page sources and a different original task. It uses
placeholder media for correspondence only. An explicit ignored native fixture
reads the existing retained two-page synthetic issue after control-slot reuse,
checks six targets and both reading pins, and compares the actual Index
checkpoint before/after export. Its native qualification result is pending.

These exports are private JSON controls, not retained review-task inputs,
verified viewing, model findings or creative approval. The owned derived
`ComicReviewControl` producer still requires a `ComicRender` parent; whole-issue
owned control generation and the subsequent model/review/fix lifecycle remain
separate work. No new admission, actor, runtime adoption, provider call, selection,
scratch retirement or payload deletion is needed for the read-only export.

The final issue's source actor was deliberately stripped of coverage for the
earlier history test. Its close preflight correctly refuses with
`NeedsRecovery/native-admission-reconcile-original`; the actual `claims.sqlite`
SHA256 stays
`c8bd3f7f6e001e80af96ed95ba15a6ca048982dd1aa520c27ba497e46c93e596`
and no close ticket is created. Evidence:
`F:/src/tig-dev-temp/reel-native-intent-final-close-preview-20261119-v1`.
That simulated state is preserved rather than reconstructing an activation or
forcing an idle marker. The healthy control actor's qualified close/reuse and
the new protected Waiting attempt remain unchanged.

Producer checklist scope follows `.roles/ROLE.md`: comics editor/story/editor
questions motivate complete issue order and exact target identity; platform and
rights/provenance checks keep viewability, translation/canon/font/voice rights,
approval and release separate. No independent review is claimed.
