# M02 compiler design and evidence boundary

**Status:** design notes prepared while M01 PR #379 qualifies. No M02 compiler
implementation, execution or qualification is claimed. Rebase on the merged M01
revision before implementation. This is part of M02, not an extra phase/PR.

## Resolve existing evidence, without opening media

The existing `reel.scene-word-timing-evidence.v1` / `WordTimingEvidence` in
`crates/reel-assembly/src/scene_authoring.rs` already binds language, cue ID,
selected-take SHA-256, sample rate, native duration and exact word intervals.
Reuse that type and `verify_word_text`; do not invent a second word-clock grammar.
The declaration's `alignment` FileRef remains an exact input reference, not a
claim that those bytes were opened by M01 validation.

M02 needs a small versioned evidence envelope at that reference to bind the exact
observed-text hash and the declared marker intervals as well as the native word
evidence. Its contract must be documented before adding code:

- Bind performance/module-local cue/language, selected script variant/hash,
  selected source hash, sample rate and duration to the exact M01 declaration.
- For recorded speech, carry the observed text and existing word evidence.
  Verify the observed UTF-8 hash and complete lexical sequence independently
  against the script and observation. This is lexical evidence, not listening,
  translation or authority approval; do not rewrite any text to make it pass.
- Bind each declared marker to explicit first/last native word indices. A word
  marker selects one word; a phrase selects a contiguous range. Resolve start/end
  from those words and compare with the declaration's sample interval. Reject
  unknown/duplicate/missing bindings or out-of-range indices.
- Engineering-signal evidence must be explicitly tagged, contain no purported
  observed speech/word transcript, and bind its synthetic clock/marker values to
  the selected signal identity. Its observed-text field in M01 is a placeholder
  identity, not proof of words. Record engineering status in the compiled output.
- Read only bounded, explicitly named control metadata through the existing
  borrowed input-provider pattern. Hash/length-check envelope bytes. Do not open
  source audio, authority or consent payloads, execute tools or infer admission.
  Declared source-byte identity remains unverified until M03.

The public M01 fixture uses synthetic pins with no payloads. M02 tests must create
actual tiny control-evidence bytes and update those alignment references rather
than pretending the old placeholder files existed. Media may still be absent:
that is a required no-media-access test, not output qualification.

## Compile sample placements and selected consumption

Flatten selected module cues in authored order, retaining speaker roles and exact
script/performance selection. Use checked integer arithmetic and half-open source
and output spans. An explicit join advances from the previous end by the added
pause or subtracts the authorized overlap. Never count recorded silence twice.
Recheck protected intervals in selected source coordinates before placement.

Produce per-edition ordered cue placements, module chapters, marker placements,
gaps/overlaps, total sample count and exact consumed-source identities. Compile
the four independent goldens from M00. Reject output beyond the 720,000-sample
fixture profile and arithmetic overflow before creating a plan destination.
Text positions derive from actual selected intervals; no English timing fallback.

Derive each edition's consumed-recipe hash from its selected module/cue/script
variants, performances/evidence, dependency requirements, join policy and output
profile. Exclude unused variants and any aggregate document/index hash. An
aggregate source-document identity may be retained separately for provenance;
changing it must not automatically invalidate an unchanged consumed recipe.
Use canonical typed serialization and explicit runtime/contract identity, not
filesystem order, physical machine paths, wall-clock timestamps or floating point.

## Runtime and report boundaries

The CLI captures the actual compiler identity; pure library tests use an explicit
fixture runtime pin. A declared pin is not native SDK runtime admission. Retained
producer compatibility and protected asset-store integration remain M05 work.
M03 must rederive/check a plan against exact controls rather than trust a caller-
editable serialized plan as an executable capability.

Keep full source-bearing compiled plans private. Portable CLI output contains
counts/status only, with separate alignment-metadata verification, source-media
verification and authority-verification fields. Neither a valid envelope nor
successful compilation makes source media or human authority verified.

Write only one requested new plan, atomically/no-clobber after all checks pass.
Failure identifies a safe relationship class and bounded indices in private
evidence, without leaking script/path/voice/authority values in normal errors.
Planning starts no media/provider process and performs no storage promotion.

## Required tests before M02 closes

1. Four exact M00 clock goldens, repeated compilation with deterministic results;
   separate host→host and host→expert→host joins; native silence retained once.
2. Valid recorded-word envelope built from controlled text/timing evidence, with
   distinct EN/ES clocks and no source media present. Mark it synthetic test
   evidence; do not describe the test as an auditioned recording.
3. Tampered envelope hash/length, wrong take/cue/language/script/observation,
   missing/extra marker binding and stale/out-of-range word/marker intervals.
4. Missing selected input, unsupported clock, context/trim/overlap failure and
   output/overflow limits. Assert diagnostic class and no new plan on failure.
5. One Spanish technical-detail revision changes only its consumed recipe; changes
   to unused variants or aggregate index do not change other edition recipes.
   M05 still owns retained-store replay and actual review invalidation.
6. Actual compile CLI, no-clobber publication, portable error/report canaries,
   and a no-media/tool/provider access assertion. New command names follow M00.

Review with Editor, Sound, Vocal Adaptation and Rights checklists; use Story for
the limits of dependency/lexical checks. Record actual commands/results and any
unresolved finding before marking M02 technically checked.
