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
checkpoint before/after export. The frozen native fixture passed in 29.55 seconds
with exit 0 on 2026-11-20. It verifies both pages and all six targets, with actual
Index checkpoint digest
`423a0f9ee4eb65e52c07f7a1338760654760106b900d92c7d4e4bf610dd639c4`
unchanged. All 132 workflow and 85 standard native tests pass (73 explicit
native fixtures excluded), including all 247 original SDK source pins. Strict
all-target Clippy passes for workflow and native with/without film.

Evidence is retained at
`F:/src/tig-dev-temp/reel-whole-issue-review-20261119-v1`.
`qualified-review.json` SHA256 is
`5e8ec110add9ef7ad70c883d3421cc826aa212c2a664bc9e3f425a26d2ca40c4`.
It pins the actual result, controls, terminal process/log and source/tool packet.
Source commit `62ccc62f4d8d1356e39241d17a1dfff1e948e952` has 1,959 verified
archived source files, source-manifest SHA256
`6f33975da8e053ad3017267e016ac5f0c247236da4cb59ed3d630556dfc46cf8`.
The frozen verifier fixture is 44,593,152 bytes with SHA256
`8ecced13100db14e232a09716f360a988db34beab6c84c024d457e24025daa6b`.
The original B9 producer runtime/profile remains unchanged; running a newer
read-only verifier is not lifecycle runtime adoption.

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
