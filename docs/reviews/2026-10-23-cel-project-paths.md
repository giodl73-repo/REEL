# Explicit cel-project path review

Base: `141bf83`. Scope: `cel_source::project`, editor source routing and candidate
writing, browser v2 metadata handling, synthetic fixtures and contracts.
Producer-applied Editor and Rights/Provenance checklists plus bounded I/O review;
no independent approval, human acceptance or runtime adoption is claimed.

| Finding | Correction and actual evidence |
| --- | --- |
| Requiring a thirteen-file folder blocked existing native layouts | An explicit descriptor routes six required native controls and up to seven optional context roles. Its root is descriptor-relative; only named files are read. Nested six-control and full-context fixtures round-trip without canonical filenames or source changes. Embedded references are preserved, not converted or crawled. |
| Content-only input locks could miss a changed file mapping | Mapped sessions/edits use v2 with an exact descriptor lock plus per-role hashes. A changed route with identical source bytes rejects. V1 cannot downgrade mapped intake. Legacy sessions/edits and browser autosave keys remain compatible; comments survive both forms. |
| Ambiguous paths or oversized metadata could produce an unusable candidate | Duplicate/unknown/missing roles, escaping/aliased/device/reserved paths and unsupported profile fields reject. Profile/path/count/raw control budgets are explicit. Encoded output budgets are checked before a new candidate directory is created; existing destinations refuse. Candidate layout emits a fresh descriptor rather than rewriting the source. |
| Filesystem observations are not a custody transaction | Named input paths reject observed symlinks/reparse points. This does not pin an adversarial namespace or certify physical storage. Interrupted writes can leave an incomplete new candidate for explicit reconciliation; no automatic cleanup, selection, eviction or runtime change occurs. |
| Private compatibility needs an actual source check | Three existing private requests pass metadata-only v2 application through a six-control profile: identity, reorder/holds and removal (40/40/39 events). Every original control hash remains unchanged. Private profiles/results are outside the public repository. No full private compiler, render or visual/audible acceptance is claimed. |

Validation: all 281 Rust tests across reel-assets/reel-assembly and nine browser
logic tests pass on Windows, plus strict all-target Clippy, formatting, ROLES and
diff checks. Nine new Rust cases cover project routing and editor integration;
three new JS cases cover v2 metadata, legacy keys and lock-map comparison. A
Unix-only symlink fixture is added for CI; it was not executed locally.

The descriptor maps existing native roles rather than defining another scene
grammar. Optional context is neither discovered nor treated as proof of a
self-contained build. Multi-cue/bilingual editing, full private compiler recheck,
media-format expansion, hostile-filesystem custody and explicit installed
runtime adoption remain separate work. No live cache access/movement, retained
media export, shared binding change or CAMINOS commit occurs in this increment.
