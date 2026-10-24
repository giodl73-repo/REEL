# Music editor browser follow-up

Producer-applied Editor and Rights/Provenance checklists; no independent review,
human visual acceptance or musical approval is implied.

An isolated, installed Edge browser exercised both native workshops against a
private retained project fixture. All editor resources were fulfilled locally;
no server or external requests were used. Private inputs, drafts and detailed
results remain project cache/LFS assets, outside this public repository.

| Finding | Change and verification |
| --- | --- |
| Committing a note field replaced its DOM controls and dropped keyboard focus | Update validation, row highlighting and the pitch roll without replacing the inputs. Actual browser Tab traversal now proceeds through pitch, start, duration and reason. Handler regression coverage also preserves input identity. |
| Underlay note-range and syllabic changes replaced the focused controls | Update the selected-card styling and validation in place. Actual range-to-syllabic Tab traversal and arrow-key selection retain focus. Structural operations still render the changed span list. |
| A long part selector overflowed the phone viewport | Bound the toolbar label and selector. Both editors fit 390-pixel and 1280-pixel document widths; the note table retains its own horizontal scrolling. |

Browser checks cover required reasons, read-only lyrics, exact source selection,
draft download/reopen, reset/undo, reload persistence and stale model/lyric/layer
imports. Browser-exported drafts pass native application, model validation and
score export verification. Original source inputs and production selections
remain unchanged. No private images or screenshots are published.

Seven JavaScript contract/handler tests, the full music crate test suite, strict
music-crate Clippy and repository role/whitespace checks pass locally. Actual
browser execution is additional producer evidence, not a replacement for those
portable synthetic regressions. Listening, musical/lyric/engraving review, human
visual acceptance and explicit installed-runtime adoption remain project gates.
