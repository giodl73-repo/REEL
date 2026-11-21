# M01 spoken-edition contract: checklist review

**Scope:** proposed M01 contract implementation, declaration fixture, CLI and
focused tests on `feat/media-m01-contract`, based on M00 main `bee948e`.
**Method:** one assistant applying Story, Editor, Sound, Platform/Audience,
Vocal Adaptation and Rights [checklists](../../.roles/ROLE.md). No independent
review, media listening, authority decision or production qualification occurred.

| Lens / finding | Disposition and evidence |
| --- | --- |
| Story / context closure could be read as factual verification | Contract/guide separate authored prerequisite checks from source truth; cycles and omitted required modules reject. Actual script/narrative approval remains unperformed. |
| Editor + Sound / multi-cue support must precede M06 | Contract supports ordered module-local cues with distinct roles now; positive test adds an expert response inside the opening module. M06 still owns assembled multi-cue qualification. |
| Sound / joins must not double recorded silence or clip protected material | Separate added-pause/overlap controls, explicit selected source intervals and protected word/breath/tail checks. Validation performs no sample placement or listening claim. |
| Vocal Adaptation / input text is not performed-word proof | Separate exact script hash and observed-text hash with referenced alignment. Declaration-only check never opens those references or claims word fidelity. |
| Editor + Vocal Adaptation / one script per language would prevent distinct executive wording | Cue-local script variant IDs let one language have separately authored audience adaptations; performances bind the exact ID and hash. A positive variant case and stale-variant rejection cover this boundary. |
| Rights / synthetic fixture could look like real consent or media | Fixture kind is engineering-signal; guide explicitly identifies synthetic pins with no asserted payloads. Result reports media/authority unverified. Scope is checked as a declaration, not granted. |
| Platform + Rights / diagnostics could leak private identifiers | Static parse/reference diagnostics; portable output exposes counts only. Canary tests cover valid report, invalid reference and actual CLI parser error output. |

Required tests: four-edition and multi-cue validation; strict/unknown/duplicate,
stale/invalid script and scope rejection; missing/cyclic context; marker, trim and
handoff failures; bounded parse; no referenced-input access/output writes; CLI
positive and redacted nonzero failure. Exact execution results are recorded in
the PR after running them, not inferred from this checklist.

Local Windows execution of `cargo test --locked --test spoken_programme --test
cue_relative --test audio_event_clocks --test episode_captions` passed 25 tests:
nine new contract/CLI tests and sixteen existing clock/caption regressions.
One existing retained-private-caption test was ignored because its explicitly
staged private inputs were not supplied; it is not reported as qualified.

No correctness/authority gap identified here is waived by declaration validity.
Audio, alignment/evidence contents, execution budgets, full-programme listening,
target usability and asset retention remain later PR obligations.
