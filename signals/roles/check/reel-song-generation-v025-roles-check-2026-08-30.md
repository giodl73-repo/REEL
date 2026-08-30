---
skill: roles-check
topic: reel-song-generation-v025
date: 2026-08-30
roles_used: [sound-designer, editor, platform-audience]
p1_count: 0
verdict: APPROVED
---

# Roles check: REEL local song-generation contract v0.2.25

## Artifact identification

- Type: Rust CLI contract, sanitized fixture, tests, and production handoff.
- Domain: music generation, exact copyrighted lyrics, local model execution,
  voice consent, provenance, private review, and later audiovisual integration.

## Role selection

- `sound-designer`: composition direction, lyrics, stems, voice identity, and
  the distinction between a generated audition and a reviewed performance.
- `editor`: duration, source order, repetition, deterministic packets, and
  downstream title-sequence timing.
- `platform-audience`: privacy, disclosure, release separation, shareable
  receipts, and eventual delivery/accessibility boundaries.

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Supplying exact lyrics cannot prove the rendered singer performed them exactly. | P2 resolved | Exact lyrics | Keep `human_listening_required` and explicitly reserve candidate transcription/listening audit for a later bound step. |
| 2 | Named listening inspirations could accidentally become imitation instructions. | P2 resolved | Engine request | Preserve `listening_references` only in human metadata and exclude them from `request.json`. |
| 3 | A named family voice requires consent distinct from manuscript rights. | P2 resolved | Permissions | Require `recorded` consent evidence for assigned identities and `not-applicable` only for an original unassigned singer. |
| 4 | Full mix alone would constrain later title mixing. | P3 | Outputs | Keep independently requestable full mix, vocal, instrumental, and stems. |
| 5 | Meter, tempo, key, prompt, negative prompt, seed, and engine parameters are sufficient for a repeatable audition brief. | P3 | Composition | Retain these fields in the private request and provenance receipt where applicable. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The song must bind to exact source order rather than a paraphrased lyric paste. | P2 resolved | Source | Hash exact lyric bytes and validate ordered, non-overlapping source ranges. |
| 2 | Repetition is a consequential editorial choice. | P3 | Lyrics | Keep `allow_repetition` explicit and false for the first BERTICA audition. |
| 3 | A 10–600 second duration window covers title music without silently accepting nonsensical requests. | P3 | Composition | Retain bounded duration and tempo checks. |
| 4 | Plan drift would make later picture synchronization unreliable. | P2 resolved | Packet check | Bind manifest, lyrics, references, and request hashes and recheck before execution. |
| 5 | The contract does not yet bind generated candidates to timeline beats. | P3 | Future execution | Add candidate/audio evidence as a separate version rather than weakening this planning boundary. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | A public-release boolean could be mistaken for approval. | P2 resolved | Permissions | Require `public_release: false` in v0.2.25 and use a separate human release decision later. |
| 2 | Exact lyrics and machine-local paths are private production data. | P2 resolved | Packet artifacts | Keep them in `request.json`; keep `receipt.json` path-free and lyric-free. |
| 3 | A local engine must not quietly egress references or lyrics. | P2 resolved | Engine/references | Require `local_only`, `offline-after-install`, `third_party_upload: false`, and `local-only` reference egress. |
| 4 | Engine readiness checks should not download weights or initiate generation. | P3 | Doctor | Keep doctor read-only and report readiness without side effects. |
| 5 | Captioning, translation, and delivery mixes are audience concerns but not generation-input concerns. | P3 | Scope | Handle them in existing REEL timeline/caption/audio contracts after a candidate is selected. |

## Synthesis

Roles reviewed: 3  
P1 blockers: 0 | P2 issues: 0 open (8 resolved during review) | P3 notes: 7

Verdict: **APPROVED**

Top finding: exact input lyrics are not evidence of exact sung output, so human
listening and later candidate evidence must remain mandatory.

Cross-role consensus: private inputs, model execution, human approval, and
public release must remain separate, hash-bound stages.

## Amendments applied

1. Excluded listening-reference metadata from the private engine prompt so
   research inspirations cannot become named imitation instructions.
2. Added assigned-voice consent validation and kept the initial original singer
   explicitly unassigned.
3. Made public release invalid in this planning contract and documented the
   separate human decision required after candidate review.
