# M06 spoken slice: role checklist and evidence map

**Status:** preparatory engineering checks on M05 PR #383 at `704cec7`.
Final local qualification and hosted checks require merged M05. Spoken review
remains **awaiting listening evidence**.

One assistant applied the six spoken-plan [role checklists](../../.roles/ROLE.md).
There are no independent reviewers or human approvals in this record.

| Lens | Finding and disposition |
| --- | --- |
| Story Director | A syntactically valid reorder can still damage the explanation. The reordered fixture explicitly claims ordering/math only. Real programmes require source/context and whole-edition narrative review. Missing required context refuses. |
| Editor | The executive opening→closing adjacency must be checked separately from the technical handoffs. Baseline executive PCM and recipes remain exact when a second detail cue is added. New internal and reordered joins get their own explicit pauses and sample goldens. Listening for pacing is outstanding. |
| Sound Designer | Recorded leading/trailing zeros remain in each source; authored gaps are added once. Every output sample is compared with an independent placement oracle. Protected overlap refuses. Signal silence and protected-region labels do not establish breaths, naturalness, masking or speech quality. |
| Platform and Audience | Temporary engineering packs reopen declaration, plan, WAV/receipt and access text. Engineering entries are labeled as signals and contain no invented word evidence. Actual headphones/mono-speaker accessibility and full delivery review remain not run. |
| Lyrics and Vocal Adaptation Editor | EN/ES use separate question scripts, takes, source durations and pauses. Exact selected text remains hash-bound in declaration/recipes; a signal is never represented as performed words. The real per-language word, translation and pronunciation gates remain open. |
| Rights and Provenance Steward | Public synthetic prose/signals and test-owned temporary directories are the whole execution scope. Existing adapters are sufficient for later authorized recording. No voice/profile authority has been supplied or inferred; no provider, private source, consent, selection, release or project adoption occurs. |

No open technical finding is waived by this review. The missing real spoken
evidence is an explicit checkpoint boundary permitted by the M06 plan, not a
passed listening gate. Animation, reconstruction and score lenses introduce no
new output requirements here: M06 adds no pictures, separation or arrangement.

## Acceptance trace

| Required behavior | Existing or new exact case |
| --- | --- |
| IDs, language/scope/script pins, context dependencies and strict declarations | `spoken_programme`: `rejects_unknown_duplicate_stale_and_unscoped_declarations`, `rejects_cycles_wrong_language_unknown_inputs_and_invalid_pins`, `rejects_context_trim_marker_and_handoff_errors` |
| Distinct EN/ES clocks, precise additions, protected trims and integer bounds | `spoken_programme`: `compilation::compiles_four_exact_sample_plans`, `compilation::rejects_context_marker_trim_overlap_and_clock_errors`, `compilation::metadata_budgets_refuse_before_input_reads_or_plan_allocation` |
| Script/recognition/evidence distinction and no claimed acoustic proof | `spoken_programme`: `compilation::lexical_metadata_is_not_audio_proof_and_keeps_language_local_positions`; `spoken_programme_media`: `checking::four_transcripts_and_chapters_follow_edition_clock` |
| Neutral PCM, explicit gap/overlap/gain/envelopes and strict sample peak | `spoken_programme_media`: `neutral_pcm_matches_independent_oracle`, `declared_gap_overlap_and_envelope_match_pcm`, `peak_limit_is_inclusive_and_one_sample_above_refuses`, `spoken_peak_rounds_down_to_the_at_most_minus_one_dbfs_boundary` |
| Invalid format/bytes, budgets, no-clobber, interruption and timeout | `spoken_programme_media`: `changed_source_and_rehashed_unsupported_format_refuse`, `aggregate_budget_refuses_before_source_access`, `failure_never_publishes_candidate`, `interruption_preserves_unrelated_files_in_reserved_directory`, `narrowed_deadline_refuses_without_publishing`, `cli_compiles_produces_and_refuses_overwrite` |
| Independent checker resists self-consistent false media, controls or text | `spoken_programme_media`: `checking::checker_reopens_media_and_rejects_rehashed_tamper`, `checking::self_consistent_changed_source_and_placement_do_not_replace_original_controls`, `checking::access_rehashes_cannot_change_words_roles_chapters_or_bindings` |
| Original retained v1, only consuming changes stale reviews, missing bytes and protection | All four `spoken_programme_history` cases; [M05 evidence](2026-11-21-m05-spoken-history-review.md) includes exact names and native/rejection scope |
| Two speakers within a module, hand-placed PCM, access and chapter goldens, preserved executive recipes | `spoken_programme_media`: `qualification::module_with_two_speakers_matches_sample_and_access_goldens` |
| Missing/reversed cues, stale omitted-detail join, protected overlap and unresolved context | `spoken_programme_media`: `qualification::multicue_adjacency_scope_and_protected_regions_refuse_before_output` |
| Authored reordering has its own clocks and cannot replace original controls | `spoken_programme_media`: `qualification::reordered_modules_need_fresh_controls_and_checked_adjacency` |
| Real CLI privacy and separate advisory review semantics | `spoken_programme_media`: `checking::portable_output_redacts_canaries`; `v0213_cli`: `independent_records_preserve_disagreement_and_explicit_resolution` |

The [qualification contract](../spoken-edition-qualification-v1.md) fixes exact
multi-cue and reordered sample counts before execution, with zero decoded-sample
tolerance and unchanged resource limits. All source fixtures and oracles are
versioned Rust/JSON; generated packs are temporary and stay outside Git.

## Execution and remaining gates

Preparatory test/lint results: pending. Final merged-parent and hosted results:
pending. Commands are recorded in the qualification contract. Each worktree has
its own Cargo target directory; no result from another worktree's executable is
accepted as evidence.

Actual EN/ES recordings, approved voice/text scope, acoustic word fidelity,
full-programme/transition listening, loudness measurement against the spoken
brief, device access review, selection, delivery and adoption: **not run**.
Later controlled-fixture engineering may use the merged interfaces but cannot
inherit any of those decisions.
