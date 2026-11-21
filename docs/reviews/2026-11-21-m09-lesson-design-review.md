# M09 lesson: source audit, checklist and qualification record

Status: implementation under qualification, on merged M08 `d48c427` / PR #391.
The lesson and admitted-reader fixtures are public synthetic data; no production
assets or musical approval have been produced. Full M09 qualification remains
required. This is one assistant applying the repository's role
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
grammar belongs in this phase. The implemented interface is
`ValidationInputs::admitted(FnMut(&Path) -> Result<Vec<u8>>)`, with operation-local
snapshots and fixed 32-object / 1 MiB-object / 16 MiB-total bounds. Existing source,
analysis, model, export and packet-check algorithms receive that explicit reader.
Filesystem entry points select filesystem mode and retain their existing support;
admitted mode rejects semantic imports and lyric/vocal layouts in this profile.

## Requirement-to-test map

The three admitted-input cases are in the music crate; the five bounded lesson
cases are in `spoken_programme_media::lesson` and explicitly run in core shard 2
on both hosted operating systems. They remain ordinary-run ignores, not missing
runtime successful skips.

| ID | Oracle / failure relationship |
| --- | --- |
| L01 | `admitted_model_and_plan_preserve_original_validation_without_disk_fallback`: same model/report/plan, false raw-source bytes refuse; callback refusal while the same relative disk paths validate. Packet equivalence is also checked by `half_frame_rounding_and_original_score_packet_callbacks`. |
| L02 | `actual_score_exports_and_bilingual_lesson_follow_pickup_repeat_goldens`: actual editable model/MIDI/MusicXML/guide, EN/ES instructions and reading bindings; exact 12000/108000/204000 intervals and source reference pin. |
| L03 | The same golden case resolves tick 720 to 36000/132000; `half_frame_rounding_and_original_score_packet_callbacks` resolves exactly 25000.5 → 25001. |
| L04 | `malformed_traversal_stale_clock_and_interrupted_publication_refuse`: duplicate/omitted/reordered/unknown sections or occurrences, missing note, outside tick, wrong PPQ/rounding, stale contract, language mismatch and unused input refuse with diagnostic-class assertions. `unsupported_admitted_layout_and_semantic_import_refuse_before_following_paths` checks early profile refusal. |
| L05 | `rehashed_false_midi_and_receipt_cannot_replace_the_original_model`: consistent altered note-on/off pitches plus rewritten MIDI/receipt hashes still fail the independent model comparison; original prepared index also refuses changed inputs. |
| L06 | `admitted_names_and_object_size_refuse_before_fallback_or_reuse` checks path, object, count and total bounds. The malformed/publication case checks zero deadline and interruption after directory reservation; the golden case proves no overwrite. `retained_lesson_reopens_without_original_score_directories_after_v2` re-prepares v1 after v2, with both original score directories removed and exact retained test providers; missing v1 bytes refuse while v2 still checks. |
| L07 | The golden case asserts the private request canary is absent from the portable summary and all authority/device/release flags stay false. The retained provider offers no filesystem view. Call inspection confirms that the lesson only reads/checks and writes its explicitly requested new index; it does not start a player, generation service, cache operation or daemon. |

## Execution

Local execution used parent `c4b55ba`; the PR was subsequently rebased cleanly
onto workflow merge `298fea5`, with all-feature Clippy and hosted qualification
required on that combined tree.

Exclusive target: `F:/tmp/reel-builds/media-m09-isolated`, Windows / Rust 1.98.0,
debug symbols disabled. Initial two lesson positives passed in 47.35 seconds;
the complete five-case explicit print-feature run passed in 118.59 seconds,
0 failed and 0 ignored. Every inherited artifact fixture retained its original
60-second bound. The three admitted-reader cases passed; music-crate all-target
Clippy passed with warnings denied, and all 65 music-crate tests passed (including
those three new cases). This is 70 distinct local passing tests at this checkpoint.

```sh
cargo test --locked -p reel-music --all-targets
cargo clippy --locked -p reel-music --all-targets -- -D warnings
cargo test --locked --all-features --test spoken_programme_media lesson:: -- --ignored --test-threads=1
```

Root all-target/all-feature Clippy passed with warnings denied. All seven existing
score/arrangement integration tests passed (four score export, three arrangement).
Formatting, actionlint, diff whitespace and local links in changed Markdown passed.
The all-feature lesson rerun passed all five cases in 166.93 seconds, with no
ignored tests in the explicit run. All five existing reading regressions passed
in 140.76 seconds. In total, 82 distinct local tests passed; the five lesson cases
were also repeated under all features. Required hosted checks remain pending.
No native asset custody or project adoption
is claimed by the retained test byte provider.

Retained disagreements/limitations: readable notation and repeat usefulness need
actual musical review; language/voice/listening/device/selection/adoption gates
remain open. A correct index is only the engineering scope described above.

## Hosted assembly package budget finding

The first M09 assembly build refused its complete source/legal bundle at 8 MiB.
The current main build did pass this gate (its later Pages deployment failed for
artifact availability); it is not the same baseline failure. Main's downloaded
package inventory totals 8,375,358 bytes including inventory and root landing,
with a 319,155-byte initial runtime. Comparable Git ZIP archives grow by 25,333
bytes for M09, before the extra source inventory entries.

The complete-package allowance is raised to 10 MiB to retain complete committed
source, tests, SDK source and legal notices. The 512 KiB initial browser runtime
cap stays fixed. Packaging and independent artifact verification still enforce
both limits and exact inventory/provenance subjects; no source or license files
are excluded to meet the budget. Final hosted package/source replay remains
required after this change. No workflow run is manually canceled.
Local packaging/independent verification passed at `14766d9`: 8,403,117 complete
bytes, 319,155 runtime bytes, 2,336 exact Git source pins and 87 served files.
This local check repackaged the existing qualified main browser binaries (the
protected assembly foundation is unchanged), not a fresh WASM build. The final
hosted assembly workflow must still rebuild, exercise the browser, replay the
delivered source outside Git and verify the transferred artifact.
First hosted Rust/FFmpeg cycle [44944853801](https://github.com/giodl73-repo/REEL/actions/runs/44944853801)
passed in 23m43s at `19924ab9b49b0321859ebf9431c5d022940c89f4`, base `298fea5`.
All five lesson tests actually ran in both core-shard-2 jobs: Linux 47.74 seconds,
Windows 45.62 seconds, no failed or ignored cases in those explicit invocations.
Existing reading qualification also passed (Linux 75.90s, Windows 64.36s).
The separate assembly build failed at the old complete-package cap. The final
head containing the documented 10 MiB allowance still requires a fresh complete
hosted cycle and assembly/source-replay qualification before merge.