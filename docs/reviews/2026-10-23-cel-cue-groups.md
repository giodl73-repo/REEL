# Native cel cue-group review

Base: `6073058`. Scope: existing native alignment-path routing, single-language
cue-local clocks, candidate application and exported-editor wiring. Producer-applied
Editor, Animation Director and Rights/Provenance checklists; these findings are
not independent review, human approval or installed-runtime acceptance.

| Finding | Correction and actual evidence |
| --- | --- |
| A global duration budget could hide drift between recordings | Explicit project v2/session-edit v3 uses native cue-local sample counts and immutable rates. Mixed-rate, off-frame synthetic cuts pass. Equal global durations with changed local coverage reject. Each cue retains at least one cel and its complete original duration. |
| Reordering must not change the recording spine | Reorder and removal stay within source cue groups. Backend checks IDs, cue identity, rate and group order; UI buttons enforce boundaries and cross-cue drag rolls back. Fit distributes holds per original cue. Graph seconds remain cue-local, preview offsets follow original cue clocks even during unfinished hold edits. |
| Shared semantic IDs are not native marker keys | Rows retain shared IDs, while measured spans and marker edits use native trigger IDs. Synthetic IDs deliberately differ. Unrelated markers survive; only removed/retained source trigger keys change. The native measured-span compiler validates each resulting cue when the candidate reopens. |
| Hash-matching all bindings could change unrelated aliases or inherit approval | Apply updates only each cue's explicitly named alignment binding. New bytes reset that binding to private-production status. A principal-approved unused alias of the original hash remains unchanged. Sources remain untouched. |
| Opening many alignments needs bounded explicit routing | The existing native cue-path map is captured once, hashed as exact source bytes, resolves filenames relative to its own parent, and must match source cue IDs. Duplicate/escaping/aliased routes reject. Limits: 128 cues, 128 KiB index, 32 MiB aggregate controls; existing per-control limits remain. Candidate layouts reopen without media discovery. Captured routing is not a custody lease. |
| Existing editors and notes must remain compatible | Frame-based folder v1 and mapped v2 requests still round-trip. V3 has separate schema/autosave identity; downgrade rejects. Comments survive remove/fit/download and native edit-request storage. Browser request import/export shares the backend 2 MiB bound. |

Validation: 287 Rust tests across reel-assets and reel-assembly, strict all-target
Clippy, formatting, ROLES and diff checks pass on Windows. Fifteen cel-editor
JavaScript tests include actual exported UI-handler execution in a synthetic
DOM (boundary buttons, drag rollback, remove/fit/undo, comments and downloaded
v3 requests). CI now includes both cel suites. This is not visual/browser
layout or audio/video acceptance; the synthetic media bytes test identity I/O,
not codecs. The existing Unix symlink fixture remains for CI, not local proof.

Private compatibility: existing mapped identity/reorder/removal requests pass
metadata-only application (40/40/39 events). A real single-cue source also passes
explicit sample-clock v3 application. All thirteen original control hashes remain
unchanged. Private scratch results stay outside this public repository. This
is not private multi-cue or full project compiler/render proof.

Remaining scope: bilingual editing, adding new assets/cues, media-format expansion,
full producer compiler/render validation, native golden selection and installed
runtime adoption. The supplied preview movie must match source cue order; this
port does not certify its audio. No live cache access/movement, media export,
server startup, shared runtime/binding change or CAMINOS commit occurred.
