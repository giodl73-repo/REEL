# M08 narrated reading: checklist and qualification record

Status: implementation in an isolated worktree; qualification in progress.
M07 merged as `1db7679d26229984c4d57389544ea9883f08bb24` in
[#387](https://github.com/giodl73-repo/REEL/pull/387), after
[CI 44933453094](https://github.com/giodl73-repo/REEL/actions/runs/44933453094)
passed in 21 minutes 7 seconds. M08 is being qualified on that merged dependency.
The complete
[M08 contract](../narrated-reading-v1.md) fixes scope and limits.

This is one assistant applying the repository's [role checklists](../../.roles/ROLE.md),
not independent reviewers or human approval. All media is public synthetic PCM
and original fixture text; no real voices, project assets, caches or providers.

## Checklist findings

| Lens | Finding / required evidence |
| --- | --- |
| Story Director | Preserve the opening/detail/closing order. Full-reading coverage requires every print chapter and each selected cue; excerpts need their own original source request. |
| Editor | Reuse independently checked edition samples and chapter boundaries. English and Spanish keep distinct durations and pauses; pagination never determines audio timing. |
| Sound Designer | Optional checked M07 mixes retain the exact dialogue parent and sample count. Reading adds no mix processing, and does not infer intelligibility from synthetic PCM. |
| Lyrics and Vocal Adaptation | Index authored UTF-8 wording and explicit byte spans, separately from transcript/recognition evidence. Declared language columns do not approve translation or pronunciation. |
| Platform and Audience | Reading text, audio chapters and one-based print pages are explicit navigation relationships. No EPUB, audiobook store, screen-reader/device or visual PDF acceptance claim. |
| Rights and Provenance | Original producer, sources, recipes, print and access packets remain pinned. Earlier packages use original retained providers; failed lookup never selects newer bytes. Detailed indexes are private, and portable summaries omit titles, paths, wording and hashes. |

No checklist lens grants text/translation/voice rights, listening approval,
creative selection or release. Independent PDF raster QA remains required by the
existing print contract and has not been performed for this engineering fixture.

## Implementation and test map

The reading module reuses the spoken compiler, M04 PCM/access checker, M07 mixer
checker and print packet rebuild. Its only new output is a small index and
ordinal UTF-8 text files. It does not copy payloads into a new asset store.
The print callback seam uses the existing parser/layout/renderer, checks all
returned pins and rebuilt outputs, and leaves the filesystem API available.

| Requirement | Test |
| --- | --- |
| Same print bytes through admitted providers; one capture per pin; refusal terminal despite original disk files existing | `admitted_snapshots_match_legacy_render_and_refusal_never_falls_back` |
| Missing/extra packet objects, false PDF and false bytes cannot pass on recomputed hashes | `declared_packet_namespace_and_rehashed_false_pdf_refuse` |
| EN/ES original wording, distinct 508800/590400 sample durations, chapter pages 2/3/4, no overwrite and false index refusal | `reading::bilingual_source_order_clocks_pages_and_authored_text` |
| Wrong language/column/recipe, omitted or reordered chapter, malformed span, missing mix, cancellation and zero deadline | `reading::stale_language_recipe_order_spans_and_missing_mix_refuse` |
| Separate Spanish source/audio v2; unchanged English text; v1 still checks with retained v1 providers; changed original bytes refuse | `reading::earlier_package_reopens_with_retained_sources_after_v2` |
| Real M07 selected mix retains chapter clock and exact output pin; false mix identity refuses | `reading::checked_layered_audio_keeps_reading_clock_and_refuses_false_parent` |
| Recomputed false PDF/print-receipt hashes, rewritten dialogue/receipt hashes and dangling visual input still refuse | `reading::rehashed_false_print_audio_and_dangling_visual_refuse` |

The five bounded media/font cases are ignored in ordinary test runs and explicitly
invoked in the existing core shard 2 on both hosted operating systems with
`--all-features --ignored --test-threads=1`. The preceding M07 run's core shard 2
completed in under ten minutes on each OS; placing the additional bounded suite
there uses existing capacity without extending the long media lane. All original
checks remain required. The first M08 hosted run must establish actual wall time.
No missing-runtime successful skip is permitted. Test-owned stores are temporary;
there is no production generation, hydration, upload or adoption.

## Execution record

Exclusive target: `F:/tmp/reel-builds/media-m08-isolated`. Debug symbols disabled;
native Windows, Rust 1.98.0. Initial print seam tests: 2 passed, none ignored.
Initial reading run: bilingual navigation and negative bindings passed; the v2
fixture exposed a test setup error (script IDs repeat per chapter). The fixture
was corrected to scope script changes by module, preserving other chapters.
A subsequent run exposed repeated full-check rebuilds exceeding the original
60-second outer fixture limit. The limit was retained. Preparation now seals the
independent result and records the exact consumed dependencies; repeat operations
revalidate all original bytes and compare actual output against that sealed
result, with independent M07 checks repeated for selected mixes. Invalid semantic
bindings also refuse before the expensive parent PCM check. This follows M07's
sealed-parent pattern and does not accept a package's rewritten hashes as proof.
The next five-case explicit run passed: 5 passed, 0 failed, 0 ignored, 137.45
seconds total, retaining each artifact fixture's original 60-second limit.
Subsequent changes add exact chapter-clock assertions and explicit negative
diagnostic checks. The first all-feature run exposed a retained-version fixture
doing four-edition parent work while retaining v1; it exceeded the same 60-second
outer limit. The reading fixture now authors exactly its two consumed technical
parents, EN and ES. The separate M07 integration retains the complete four-edition
parent. No execution limit or required behavior was relaxed.

The corrected explicit **all-feature** reading run passed all five cases in
135.27 seconds, none ignored in that invocation. All original fixture limits held.
The focused existing regressions passed 64 distinct cases: 20 declarations,
24 media/checker cases, four retained-history cases, nine semantic-clock cases,
six Sonic cases and one review CLI case. One existing tamper fixture hit its
60-second limit in the parallel run (60.35 seconds overall); rerunning that exact
unchanged case alone passed in 29.18 seconds. The other 63 cases passed without
that timing failure. No original regression was edited or its limit increased.
The native print crate passed seven tests: two new admitted-input cases and five
existing collection cases. This is **76 distinct locally passing tests**, not a
claim that every required hosted runtime case has run locally.

Print-crate all-target Clippy passed with warnings denied. Root all-target,
all-feature Clippy passed again after the final fixture adjustment and after
separating the authored request and generated index schema IDs. The focused
bilingual test then passed in 17.41 seconds, asserting both wire IDs and refusal
to use an index schema for a request. This rechecks the final serialization
change without repeating unrelated media work. Formatting, changed-document
links, actionlint 1.7.12 and diff checks passed. M08 current-head hosted
qualification remains pending.

The principal commands are:

```sh
cargo test --locked --all-features --test spoken_programme_media reading:: -- --ignored --test-threads=1
cargo test --locked -p reel-print --all-targets
cargo clippy --locked -p reel-print --all-targets -- -D warnings
cargo clippy --locked --all-targets --all-features -- -D warnings
```

The legacy Sonic real-render case and four M07 runtime cases retain their
separate explicit hosted invocations; ordinary-run ignores do not close them.
M08's current-head hosted checks are still required; the M07 merge is recorded above.

Every operation retains a cooperative maximum 30-second deadline; source/text,
print, producer and package limits are fixed in the contract. Trusted synchronous
I/O must return, and asset providers must retain their own read guards. These
checks do not claim OS-level preemption or global custody.
