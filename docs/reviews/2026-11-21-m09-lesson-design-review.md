# M09 lesson: source audit and pre-implementation checklist

Status: preparatory design only, on merged M08 `d48c427` / PR #391. No M09 runtime,
assets, musical approval or tests have been produced. M09 implementation and its
own qualification remain required. This is one assistant applying the repository's role
checklists; it is not independent review or authorization.

The [lesson contract](../lesson-rehearsal-v1.md) fixes the hand-derived pickup,
repeat occurrences, clocks, bounds and required rejection cases.

| Lens | Finding / disposition |
| --- | --- |
| Platform and Audience | Keep complete checked EN/ES explanation, written instructions and the exercise clock distinct. An authored traversal/index does not establish a working player or accessible device experience. |
| Score and Arrangement | Native editable pickup fields exist; repeat traversal/barlines do not. Preserve the written model and checked exports; use explicit `A-pass-1` / `A-pass-2` exercise occurrences. Do not claim repeat engraving, new arrangement or session-ready notation. |
| Music Reconstruction | Reuse `music_score::tick_to_sample` / `native_note_clock` integer conversion. Native guide is 48 kHz mono PCM16, while spoken dialogue is stereo PCM24. Apply each declared source clock and test the exact half-frame case. |
| Vocal Adaptation | Authored wording, recognition evidence, original narration and written exercise instructions remain separate. Never describe synthetic signals as proof of spoken/sung words or infer a translation/performance approval. |
| Sound Designer | The existing square-wave guide is for timing/pitch rehearsal. A virtual repeat does not establish an audible join, mix quality, performance master or listening pass. |
| Rights and Provenance | Existing model validation checks source/analysis/provenance; editor capture alone does not. Preserve actual observed/inferred evidence and correction decision pointers, with no invented human correction or authority. Keep detailed identities private and use fixture-only canaries. |

## Input-boundary decision before runtime work

`reel_music::model::validate` follows source/analysis paths; `music_score::check`
opens the original model, plan, receipt and export files. `arrangement_candidate`
reuses that same checker through filesystem paths. The protected note/underlay
editor operates on raw authoring bytes and explicitly does not validate complete
musical/source lineage. The current native `ProductionContext` records declared
metadata and explicitly disclaims execution/availability proof.

Therefore the selected direction is a narrow bounded byte-callback seam inside
the existing validators/checker, preserving legacy filesystem behavior and exact
original names. It must independently verify supplied bytes and use the same
musical algorithms. A callback refusal must not retry disk paths. No generic
filesystem projection, store, cache inventory, automatic hydration or new score
grammar belongs in this phase. The exact interface and its first targeted tests
remain the next implementation task; this review does not assert they exist.

## Planned evidence IDs

These IDs are requirements, not existing passing tests. The implementation PR
must replace each row with exact runnable test names/commands and actual results.

| ID | Oracle / failure relationship |
| --- | --- |
| L01 | Same validation/export result through legacy files and admitted bytes; false bytes and terminal refusal while original disk files exist |
| L02 | Actual editable pickup, MIDI/MusicXML/guide and EN/ES reading bindings; the 12000/108000/204000-sample traversal table |
| L03 | Tick 720 resolves differently for each A occurrence; PPQ 960 / tempo 500010 / tick 1000 resolves 25000.5 → 25001 without floats |
| L04 | Missing/duplicate occurrence, unknown section/note, incompatible pickup, reversed/outside ranges and unsupported timebase/rounding refuse |
| L05 | Changed model/note/tempo/audio/export/reading and rehashed false receipts cannot replace original control identities |
| L06 | Declared bounds, deadline/cancellation, occupied output and prior-version reopening preserve original assets and prevent a partial final index |
| L07 | Portable count summaries omit fixture-only private canaries; planning/checking starts no provider process, player, daemon, upload or cache mutation |

Retained disagreements/limitations: readable notation and repeat usefulness need
actual musical review; language/voice/listening/device/selection/adoption gates
remain open. A correct index is only the engineering scope described above.
