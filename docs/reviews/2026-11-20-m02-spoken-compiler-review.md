# M02 spoken compiler: draft checklist review

**Status:** draft review; local preparatory checks may run against exact M01
`be9049b`, while final qualification waits for merged M01 and a rebase.
**Method:** one assistant applying Editor, Sound, Vocal Adaptation, Rights and
Story checklists from [.roles](../../.roles/ROLE.md), plus Platform/Audience for
the private-plan/portable-output boundary. No independent panel, audio audition,
language approval, consent or runtime admission is claimed.

| Lens / finding | Draft disposition | Evidence still required |
| --- | --- | --- |
| Editor / exact joins and selective consumption | Four independent sample goldens; authored order with explicit pause/overlap; canonicalized requirement sets; scope projected to the consuming edition, with unused variants/other allowed IDs excluded | Execute goldens, registry reordering, unused-input, extra-edition allowlist and Spanish-detail revision tests |
| Sound / signal clocks could be mistaken for observed speech | Engineering envelopes have no observation/word binding; word evidence is separately tagged and shifted without retiming | Run no-media/provider access cases and native-word/marker rejection cases; actual PCM proof remains M03 |
| Editor + Sound / the 15-second engineering limit did not cover the 30-second spoken brief | Explicit engineering/spoken profiles preserve the first golden cap and permit the stated brief without stretching; profile choice enters recipe identity | Execute the longer controlled recorded-clock case; actual cadence, target duration and intelligibility still need listening |
| Vocal Adaptation / recognized spelling must not silently replace transcript text | Retain exact submitted observation on each cue; output positioned recognizer strings under `word_evidence`, not approved transcript text | Execute the language-local observation/timing tests; actual word fidelity/translation/listening remain unperformed |
| Rights / checked metadata could imply audio or authority clearance | Portable result distinguishes alignment-metadata checks from media/authority verification; full plans stay private | Canary and actual CLI failure/no-clobber tests; no actual authority/admission decision is granted |
| Story / dependency and lexical checks do not prove meaning | Reuse M01 declaration closure; keep source/observed/recognized text identities separate | Actual source/narrative review for any real edition, with no inferred endorsement |
| Platform/Audience / bounded diagnostics and practical access | Static classes plus bounded metadata/edition/cue indices; private text absent from portable summaries | Execute CLI/report cases and output cleanup checks; no device/accessibility qualification claimed yet |

The draft reuses native `WordTimingEvidence` and `verify_word_text` instead of
adding a new word-clock grammar. Its envelope adds exact observation, marker
binding and selected-declaration correspondence. Compiler identity is explicit;
source audio/authority payloads and native storage/runtime admission are not read.

Preparatory commands may run on the recorded stacked base; final qualification
must confirm the merged-base revision after M01 merges and the branch rebases:

```sh
cargo test --locked --test spoken_programme --test cue_relative --test audio_event_clocks --test episode_captions
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Preparatory Windows `cargo test --locked --test spoken_programme` passed all
20 tests on the stacked draft based on M01 `be9049b` (nine declaration/CLI cases
and eleven compilation cases). This is not merged-base or hosted qualification.
Preparatory all-target/all-feature Clippy also passed with warnings denied.
Required hosted checks, the final merged-base revision, private/public evidence
separation and unresolved findings must be recorded before M02 is marked
technically checked. This review introduces no media execution,
cache initialization, hydration, provider use, production selection or adoption.
