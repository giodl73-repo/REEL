# M04 independent checker: checklist review

**Status:** local checks rerun on merged M03 `a8b481b`, [PR #381](https://github.com/giodl73-repo/REEL/pull/381), after its required hosted checks passed. M04 hosted checks remain pending. The
[checking/access contract](../spoken-edition-check-v1.md) precedes these local
controlled-signal tests. No merged M04 capability is claimed.

**Method:** one assistant applying Sound, Platform/Audience, Vocal Adaptation and
Rights checklists from [.roles](../../.roles/ROLE.md), with Editor for handoffs
and Music Reconstruction for the independent sample oracle. No independent
human reviewers, listening, language approval or release authority are invented.

| Lens / finding | Disposition |
| --- | --- |
| Sound / self-consistent hashes can hide wrong audio | Reopen complete sources/output and separately calculate every selected, gained, faded and mixed sample. No producer or PCM-window transformation is called. A changed WAV with updated container/PCM hashes still fails. |
| Editor + Sound / alternate controls can replace authored intent | A real producer builds alternate-source and alternate-pause candidates with all hashes recomputed. The checker still requires the original declaration/plan and rejects the alternate relationships. Runtime, order and externally authored processing are checked separately. |
| Platform/Audience / text access must describe checked media | Transcript/chapter JSON and UTF-8 transcripts use the checked sample clock, exact selected script and explicit pause/overlap annotations. Engineering signals never claim spoken words. Actual text files are reopened before access-package closure. |
| Vocal Adaptation / recognized text is a different layer | Selected-script transcript remains separate from ordinal-speaker word evidence with its own exact shifted intervals. EN/ES timing fixtures differ; deliberately lowercased recognition tokens must not rewrite the selected script. No acoustic word fidelity, translation approval or naturalness is inferred. |
| Rights / exact evidence is private | Detailed plans, receipts and access data remain local private artifacts. Portable summaries have counts/fixed status only; real CLI success/failure canaries protect local paths, project/source IDs and input values. Speaker labels expose ordinal roles, not voice/provider identities. |
| Rights + Platform/Audience / failed access attempts can retain a prior success flag | Each verification/publication attempt resets access status before work. Tampering, missing input, existing destination, timeout and interruption yield no success/promotion; unrelated and earlier files survive. |
| Music Reconstruction / numeric profile convention | PCM24 full scale follows the existing decoder's division by 2^23. The spoken rail is 7,476,354, with exact positive boundary coverage; this does not qualify loudness or true peak. |

Story/Animation/Score are outside this PR's authored output: no narrative,
pictures, score, player or arrangement is created. Their later production gates
remain separate. Listening, actual word fidelity, consent, selection and native
retention/admission are also unperformed.

## Requirement-to-test map

All new cases are under `checking::` in `spoken_programme_media`:

| Requirement | Test |
| --- | --- |
| Independently reopened PCM and rehashed false waveform | `checker_reopens_media_and_rejects_rehashed_tamper` |
| Four edition clocks, exact script, ordinal roles, distinct EN/ES word evidence, gaps and chapters | `four_transcripts_and_chapters_follow_edition_clock` |
| Word/text/role/interval/chapter/source mutation, including updated transcript pin | `access_rehashes_cannot_change_words_roles_chapters_or_bindings` |
| Fully recomputed alternate source and placement cannot replace original controls | `self_consistent_changed_source_and_placement_do_not_replace_original_controls` |
| Runtime/order/source/processing mutation and missing media/receipt | `runtime_source_order_and_processing_tamper_refuse` |
| Interrupted access publication, foreign-file preservation, no overwrite and zero deadline | `access_publication_interruption_and_existing_output_preserve_files` |
| Independent rational gain, signed division, fades and overlap; hand-calculated sample probes | `independent_checker_accepts_declared_envelopes_and_overlap` |
| Spoken peak rail under existing PCM normalization | `spoken_profile_check_uses_the_native_pcm_full_scale` |
| Real CLI creation, reopened text verification, failure and private canaries | `portable_output_redacts_canaries` |

Earlier shared-target runs are discarded as qualification evidence after
switching worktrees reused a stale test executable. Fresh verification in
`F:/tmp/reel-builds/media-m04-isolated`, exclusive to this worktree, passed 21 media
cases, 20 declaration/compiler, nine clock and seven shared PCM/WAVE cases: 57 total,
none ignored. The merged-parent media target completed in 35.51 seconds and includes
M03's added peak/channel-layout regressions. Recognition tokens deliberately
differ from the canonical script. All-target/
all-feature Clippy with warnings denied passed. Formatting, local links and
`git diff --check` passed. Required hosted Windows/Linux qualification remains pending.

The existing private receipt gains strict deserialization with owned schema/
status strings; its serialized JSON contract stays unchanged. Shared helpers are
limited to parsing, validation, bounded verified reads and no-clobber writes;
sample processing is separately implemented. Word-evidence fixtures deliberately
attach synthetic lexical metadata to signals solely to exercise packaging. They
are never evidence of an actual spoken demonstration or human acceptance.
