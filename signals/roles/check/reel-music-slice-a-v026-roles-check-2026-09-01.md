---
skill: roles-check
topic: reel-music-slice-a-v026
date: 2026-09-01
roles_used: [music-reconstruction-engineer, score-arrangement-director, lyrics-vocal-adaptation-editor, rights-provenance-steward, sound-designer, editor, platform-audience]
p1_count: 0
verdict: APPROVED
---

# Roles check: `reel-music` Slice A v0.2.26

## Artifact identification

- Type: Rust workspace crate, CLI integration, strict contracts, synthetic
  fixtures, tests, and implementation documentation.
- Domain: immutable audio identity, repair planning, timebase precision,
  authority and egress, privacy, portability, and later music transformation.

## Role selection

- `music-reconstruction-engineer`: primary engineering contract and evidence.
- `score-arrangement-director`: musical timebase and explicit future score
  boundary.
- `lyrics-vocal-adaptation-editor`: exact-byte source discipline and explicit
  exclusion of lyric transformation from this slice.
- `rights-provenance-steward`: authority, decisions, privacy, egress, and
  execution separation.
- `sound-designer`: decoded signal, future seam/tail evidence, and claim scope.
- `editor`: range, operation, overlap, lock, and reversibility semantics.
- `platform-audience`: Windows/Linux portability and local/shareable output
  distinction.

Story and animation roles are not selected for this implementation pass because
Slice A contains no narrative adaptation, picture timing, or rendered media.

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Exact container and decoded identity are independently represented, with equality provable for the raw-PCM foundation. | P2 resolved | `source.rs` | Retain both hashes when container decoders arrive. |
| 2 | Neutral planning covers and locks the complete source, and checking requires exact candidate hash and byte length. | P2 resolved | `neutral.rs` | Keep rendering separate and bind its later decision list to this plan. |
| 3 | Repair plans cover every sample exactly once as changed or locked and reject lock trespass and overlapping mutating operations. | P2 resolved | `repair.rs` | Preserve this invariant when operation groups are designed. |
| 4 | Insert/replace assets initially lacked enough decoded-format evidence. | P2 resolved | `AssetRange` | The amended contract now verifies raw/decoded hashes, byte count, sample rate, channels, format, range, and destination length. |
| 5 | Raw PCM is intentionally narrower than real source-container intake. | P3 | v0.2.26 docs | Add container decoding only behind a versioned adapter with normalized fingerprint test vectors. |

## Score and Arrangement Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The explicit PPQ and rounding mode create a usable future score boundary without pretending a sample/tick map already exists. | P3 | `time.rs` | Add tempo-map anchors and sample/tick mapping with the corrected-model slice. |
| 2 | The implementation makes no claim to notation, playability, recognition, or arrangement approval. | P2 resolved | Scope | Keep those claims behind editable score evidence and human A/B review. |
| 3 | `extend-bars` is typed but rendering semantics are intentionally absent. | P3 | `Operation` | Specify meter, tempo-map, source material, and cadence behavior before Slice B executes it. |

## Lyrics and Vocal Adaptation Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | No canonical, performed, or translated text is present in the new fixture or contract. | P2 resolved | Fixture/privacy boundary | Preserve text-layer contracts for the later language slice. |
| 2 | The pre-existing exact-lyric fixture now has pinned LF bytes and passes unchanged hash checks on Windows and Linux. | P2 resolved | `.gitattributes` and v0.2.25 fixture | Keep exact bytes authoritative; never normalize user content silently at validation time. |
| 3 | Lyric correction and language adaptation remain visibly deferred. | P3 | v0.2.26 docs | Require separate hashes, authorities, underlay, prosody exceptions, and performed-word evidence before implementation. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Source authority includes namespace, artifact/content identity, status, roles, and immutable decisions. | P2 resolved | `AuthorityRef` | Expand authority types only through additive versioned contracts. |
| 2 | Reviewed/approved/selected/released statuses could initially be asserted without decision evidence. | P2 resolved | Authority and repair review validation | The amendment now requires decision references for those statuses and rejects duplicate decision IDs. |
| 3 | The foundation requires private, network-denied, no-upload egress and invokes no adapters or external processes. | P2 resolved | `source.rs` and CLI | Preserve side-effect-free validation and planning. |
| 4 | Local validation reports expose IDs, paths, and hashes and must not be treated as exchange receipts. | P2 resolved | Report schemas | Every new report now states `shareable: false`; design redacted receipts separately. |
| 5 | No consumer-private material appears in code, tests, fixture, or docs. | P2 resolved | Repository census | Keep BERTICA acceptance evidence outside REEL and exchange only generic receipts. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Sample format, rate, channels, sample count, and exact signal identity are sufficient for a deterministic raw-PCM foundation. | P2 resolved | Source contract | Preserve these facts through every rendered derivative. |
| 2 | The planner does not claim that structural validation proves an inaudible seam or musical correction. | P2 resolved | Documentation | Slice B must measure and bind boundary, phase, ambience, loudness, spectral, and tail evidence. |
| 3 | Crossfade and tail operations are typed but cannot yet overlap another operation. | P3 | Repair grammar | Add explicit operation-group composition rules before combined replace/crossfade execution. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Integer half-open ranges eliminate ambiguous inclusive endpoints and floating-point edit identity. | P2 resolved | `SampleRange` | Keep all executable edit boundaries sample-based. |
| 2 | Every changed envelope must be fully covered by operations and every unaffected sample must be locked. | P2 resolved | Repair validation | Retain exact coverage in every schema version. |
| 3 | Operation IDs are unique and mutating overlap is rejected rather than resolved by undocumented order. | P2 resolved | Repair validation | Introduce ordering groups only with a canonical resolved decision list. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The checked-in canonical hash and complete test suite pass on Windows and WSL/Linux. | P2 resolved | Portability evidence | Add the same commands to CI when this branch is integrated. |
| 2 | Raw fixtures are binary and text fixtures are LF-pinned, preventing platform checkout drift. | P2 resolved | `.gitattributes` | Keep fixture encoding policy explicit for every hash-bound format. |
| 3 | CLI commands are additive and old `song-*` commands remain compatible. | P3 | Root CLI | Preserve command/schema bytes if generation moves into `reel-music` later. |

## Synthesis

Roles reviewed: 7
P1 blockers: 0 | P2 issues: 0 open (20 resolved/passing) | P3 notes: 8

Verdict: **APPROVED**

Top finding: Slice A now proves exact source, neutral candidate, changed-region,
locked-region, authority, and local-output boundaries without claiming repair
execution or creative approval.

Cross-role consensus: the implementation is correctly narrow. Raw PCM and
planning are sufficient for the foundation; decoding, rendering, acoustic
evidence, notation, language, and arrangement must remain later reviewed
slices.

## Amendments applied

1. Strengthened insert/replace asset validation with decoded identity, byte
   count, format/timebase compatibility, range bounds, and equal destination
   duration.
2. Required immutable decision evidence for reviewed, approved, selected, or
   released status and rejected duplicate decision identifiers.
3. Marked all v0.2.26 CLI reports `shareable: false` and added canonical neutral-
   plan identity so local evidence cannot be mistaken for a redacted receipt.
