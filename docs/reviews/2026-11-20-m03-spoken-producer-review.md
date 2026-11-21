# M03 spoken producer: checklist review

**Status:** local checks rerun on merged M02 `cc099b4` (#380), after its required
hosted qualification passed. No M03 hosted, independent
media-checker, retained-publication or listening pass is recorded here.

**Method:** one assistant applying the [REEL checklists](../../.roles/ROLE.md).
Sound, Editor and Rights are the planned M03 lenses; Music Reconstruction covers
the shared PCM helper and Platform/Audience covers the evidence boundary.
This is not an independent panel or human production approval.

| Lens / concern | Disposition and required evidence |
| --- | --- |
| Editor: scope and sample placement | Four hand-calculated edition goldens compare the complete decoded output, including recorded silence and added gaps. A separate fixture changes one explicit adjacency to an admitted overlap. No picture dependencies or inferred bridge are introduced. |
| Sound: neutral assembly before processing | Identity policy preserves selected PCM bytes exactly. Rational gain precedes the existing integer envelope; signed rounding, endpoints and sum peaks have explicit oracles. No automatic gain, stretch, channel conversion or lossy codec is included. |
| Music Reconstruction: shared helper regression | Reuse the existing bounded RIFF parser and PCM-window transform. Old float/integer decoding and original retained PCM-window tests remain required alongside the new gain case. Exact native PCM does not require an external decoder; other formats refuse. |
| Sound: two channels could carry a different extensible layout | The exact PCM24 path requires explicit left/right mask 3 on extensible WAVE. A dedicated byte fixture accepts that mask and rejects unspecified/other masks. The existing broad song decoder retains its prior behavior. |
| Sound + Rights: failure can resemble a candidate | The exclusive new output directory uses a final no-clobber receipt marker. Check cancellation after staged writes. Refuse altered pins, incompatible containers, protected fades, peak/budget/deadline overruns and edited plans. Interrupted owned files retire; unrelated files and older candidates survive. Concurrent writers are outside this destination contract. |
| Rights: runtime/source verification can imply authority | Recompile with the current executable, then verify complete source bytes/frame counts. This establishes current control/byte bindings only. Authority payloads, native admission and independent checker decisions remain unverified. Historical/runtime admission is M05 work. |
| Platform/Audience + Rights: receipts contain private references | The detailed plan/receipt remains local private evidence. CLI summaries contain counts and fixed status only; normal success/failure output must exclude fixture canary names, project IDs and local paths. No private evidence is published by this tool. |

Story, Animation, Score/Arrangement and Vocal Adaptation are not additional M03
production acceptance lenses: no story/text, picture, score or performance is
authored/approved in this bounded signal producer. Their M06/M09/M10 decisions
remain separate; this review does not inherit them.

## Requirement-to-test map

| Requirement | Runnable test / oracle |
| --- | --- |
| Exact neutral four-edition PCM | `neutral_pcm_matches_independent_oracle`: independent full-stream sample goldens and selected/processed hash equality |
| Declared overlap, gain and fades | `declared_gap_overlap_and_envelope_match_pcm`: independently calculated signed integer stream |
| Input identity and format | `changed_source_and_rehashed_unsupported_format_refuse`: changed bytes fail pins; rehashed incompatible WAVE fails format checks |
| Control binding and protected intervals | `modified_plan_and_protected_fade_refuse`: edited placement cannot match recompiled controls; protected word cannot be faded |
| Inclusive engineering peak | `peak_limit_is_inclusive_and_one_sample_above_refuses`: 4,194,303 passes; 4,194,304 refuses without replacing the earlier candidate |
| Spoken peak normalization | `spoken_peak_rounds_down_to_the_at_most_minus_one_dbfs_boundary`: floor(2^23 × 10^(-1/20)) = 7,476,354 passes; 7,476,355 refuses; the existing decoder's full-scale convention remains unchanged |
| Aggregate preflight | `aggregate_budget_refuses_before_source_access`: individually legal declarations exceed the aggregate cap before unavailable source access |
| Cooperative timeout | `narrowed_deadline_refuses_without_publishing`: zero host budget takes the actual deadline refusal path |
| Strict processing | `processing_targets_gains_and_envelopes_are_strict`: unknown/duplicate targets, invalid gain/envelope and unknown JSON fields refuse |
| Interrupted publication and no overwrite | `failure_never_publishes_candidate`, `interruption_preserves_unrelated_files_in_reserved_directory`: no final marker, owned cleanup and foreign-byte preservation |
| Actual CLI/privacy | `cli_compiles_produces_and_refuses_overwrite`: same executable compiles/produces, safe summary/canary-free diagnostics, second invocation preserves every candidate byte |
| Shared PCM/WAVE compatibility | Four PCM-window and three WAVE unit cases, including signed endpoints, clipping, float-bit round trip, damaged containers and exact extensible channel masks |

Commands and limits are in the [production contract](../spoken-edition-production-v1.md).
All fixtures are tiny test-owned signals. No asset cache, provider, source voice,
production recording, upload or native publication is used.

## Execution record

Merged-parent Windows execution passed twelve media integration tests, twenty
declaration/compiler cases, nine clock regressions and seven PCM/WAVE helper unit
tests: 48 tests total, with no ignored case in these selected targets. Fresh
verification used `F:/tmp/reel-builds/media-m03-isolated`, exclusive to this
worktree, after a shared-target stale executable was detected in preparatory
work. The final media target completed in 12.20 seconds after adding the spoken rail regression and
the measured 60-second fixture budget. Trusted synchronous I/O must return;
cooperative deadlines and measured budgets are not operating-system preemption.
All-target/all-feature Clippy with warnings
denied, formatting, local links and `git diff --check` passed. Required M03 hosted
Windows/Linux qualification remains pending; local results are not substituted
for those gates. Earlier shared-target and stacked runs are not substituted for
this isolated merged-parent evidence.

Independent reopening/text evidence belongs to M04. Actual speech wording,
natural handoffs, voice scope, language approval, listening, selection and adoption
are unperformed and cannot be closed by these signal tests.
