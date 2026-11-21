# M10 semantic companion: pre-implementation review

Status: design only; M09 PR #394 must qualify and merge before an M10 PR. This is one assistant applying `.roles` checklists, not independent review or creative approval. The [contract draft](../semantic-visual-companion-v1.md) records source inspection and the hand-derived EN/ES example.

## Scoped API decision

Retain the original `checker::CheckRequest`, source/candidate providers and exact visual/access pins. The independent checker already recomputes the plan before checking audio. A read-only accessor on its sealed `CheckedPackage` is the preferred seam; do not accept an arbitrary caller-created `CompiledEdition` as checked evidence. Reuse `spoken_phrase_sample` and the existing browser source-clock adapter. No new phrase matcher, audio renderer, compositor or cache facade.

Before runtime implementation, finalize concrete borrowed types and identify exactly which original pins are rechecked on reopen and after cancellation callbacks. No whole-filesystem fallback or self-reported receipt may establish the original expected subject. A checked metadata relationship does not prove that a recording acoustically contains words.

## Proposed fixed qualification profile

These are proposed limits, not implemented promises. Confirm them against existing worker invariants before its first fixture:

- One original SVG, two authored event IDs, one or two selected editions, at most two language bindings per event.
- 48 kHz stereo PCM24 checked programme, 24 fps, at most ten seconds per edition; full audio duration must equal an integer frame count. No implicit padding, trimming, fades or resampling.
- SVG/control resources at most 64 KiB each; source clock at most its existing 64 KiB; clock evidence at most its existing 1 MiB. Original spoken checks retain their own declared bounds.
- At most 240 picture frames and 640×360 canvas pixels. The synthetic pixel-oracle fixture uses a much smaller canvas and shorter clock; it makes no readable-device or spoken-performance claim.
- Existing owned Windows worker: 256 MiB browser / 1 GiB encoder memory, at most eight browser / two encoder processes, 64 MiB workspace, 16 MiB output, 30-second cooperative deadline, provided these satisfy the existing host admission contract. No invocation without explicitly bound programs and held inputs.
- Index publication uses the existing owned no-clobber writer and final marker. No production cache, daemon, hydration, census, upload, deletion or player starts.

If a proposed bound conflicts with the current worker, record the conflict and revise this contract before execution. Do not enlarge it silently to pass a fixture.

## Requirements and expected test names

| ID | Planned executable case | Independent expectation |
| --- | --- | --- |
| C01 | `bilingual_phrase_clock_uses_sealed_checked_package` | Same two event IDs; exact EN phrases at samples 48000/96000, frames 24/48; ES at 60000/120000, frames 30/60. Original take, language, script, observation and marker ranges must match through existing compiler/phrase resolution. Synthetic metadata is labeled as such. |
| C02 | `changing_english_take_moves_only_english_companion` | A replacement EN recording/alignment changes EN consuming hashes/times and leaves ES/event intent unchanged. Keep all old and new subjects distinct. |
| C03 | `real_native_bilingual_svg_transitions_keep_exact_pcm` | Explicit enrolled browser/FFmpeg fixture inspects independently decoded frames before/between/after both transitions and exact decoded PCM. Keep synthetic signal classification; do not manufacture recorded-speech evidence. |
| C04 | `ambiguous_phrase_stale_subject_and_missing_visual_refuse` | Diagnostic classes for repeated phrase without occurrence, unknown marker/edition, wrong language/take, stale alignment/audio, missing SVG, unsupported geometry and nonintegral duration. |
| C04 | `altered_clock_receipt_cannot_replace_original_alignment` | Rewritten clock and receipt hashes still fail against original caller-held controls and regenerated expected anchors. |
| C05 | `companion_text_access_preserves_language_and_signal_basis` | Exact Spanish/English text and independent transcript/caption access; engineering signals never become spoken captions. A pixel fixture does not establish readable typography or human accessibility acceptance. |
| C06 | `interrupted_companion_publication_is_owned_and_no_clobber` | Cancellation after reservation leaves no committed marker, existing output is untouched, deadline/bounds failures do not start work, only owned intermediates are cleaned. |
| C06 | `earlier_companion_reopens_with_original_inputs_after_replacement` | Re-prepare/reopen retained v1 after v2 with original source directories absent; exact retained callbacks suffice, missing v1 bytes refuse, v2 still checks. Test provider is not a production store/custody claim. |
| C07 | `portable_companion_summary_omits_private_subjects` | Canary phrases, titles, speaker/authority names, paths and private hashes absent; acoustic/linguistic/authority/device/release flags remain false. |

The names above are planned, not executed tests. Runtime qualification must explicitly run real browser tests in CI; no successful skip or ordinary ignore establishes coverage. Final source and test names must agree before merge.

## Checklist findings and remaining gates

| Lens | Finding |
| --- | --- |
| Animation | Existing SVG/frame-event pipeline is sufficient for three deterministic visual states. Actual decoded frames must prove two visible transitions. |
| Story | Event IDs preserve intent across independently phrased languages. Source text and narrative order stay upstream. |
| Editor | Keep leading silence, tails, language-specific pacing and the original audio signal; derive frame boundaries once from integer samples. |
| Sound | Independent PCM checking must stay upstream of picture. Do not make an audio-only package depend on visual sources. |
| Platform/Audience | Export text access alongside the companion. Font size, reading pace and real device acceptance remain unperformed. |
| Vocal Adaptation | Reuse exact source/word matching; timing is not translation or listening approval. Need actual source recordings for a real spoken checkpoint. |
| Rights/Provenance | Keep immutable scopes and private subjects separate from portable counts/flags. No inferred voice, language, performance or release authority. |

Unused score/arrangement and music-reconstruction lenses: no score alteration, transcription, note clock, stem repair or musical arrangement occurs in M10. The lesson score remains its already-checked upstream reference.