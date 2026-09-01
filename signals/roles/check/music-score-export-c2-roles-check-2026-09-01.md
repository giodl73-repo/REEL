---
skill: roles-check
topic: music-score-export-c2
date: 2026-09-01
roles_used: 4
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL role review: music score export C2

## Artifact identification

- Type: Rust implementation, versioned data contract, CLI, test fixture, and
  production documentation.
- Domain: music reconstruction, notation interchange, rehearsal audio,
  provenance, privacy, and technical verification.
- Reviewed scope: `reel.music-score-export-plan.v0.1`,
  `reel.music-score-export-receipt.v0.1`, MIDI/MusicXML/WAV adapters, re-import
  comparisons, CLI, tests, and v0.3.1 documentation.

## Role selection

- Music Reconstruction Engineer: exact model lineage, tick/sample conversion,
  deterministic adapters, and independent round trip.
- Score and Arrangement Director: editable notation, retained musical
  structure, and the boundary before session-ready scoring.
- Sound Designer: rehearsal-guide behavior and claims about audible output.
- Rights and Provenance Steward: local/private receipt design, egress policy,
  and separation of technical success from authorization.

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | MIDI and MusicXML are both compared back to the exact validated model for duration, maps, form, notes, and lyric bindings. | P3 | `src/music_score.rs` round trip | Retain the independent parsers as adapters expand. |
| 2 | Tick-to-sample conversion uses integer accumulated tempo weights and declared half-up sample rounding; oscillator frequency uses a fixed integer table. | P3 | rehearsal guide | Add cross-OS golden hashes if additional waveforms appear. |
| 3 | C2 rejects unsupported PPQ, voice count, and overlapping same-voice notes before publication. | P3 | `export::build` | Add a future typed capability report if polyphonic notation support broadens. |

## Score and Arrangement Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Parts, voices, forward/backup timing, notes, tempo directions, meter declarations, and form rehearsals remain editable instead of collapsing to PDF/audio. | P3 | MusicXML adapter | Test import in at least two notation applications before calling it session-ready. |
| 2 | The single implicit non-controlling measure preserves ticks but intentionally does not provide engraved bar layout. | P3 | MusicXML limitation | Add bar construction, ties, pickups, and meter-change layout in a later scored-notation slice. |
| 3 | Lyric layer identities survive, but syllable-to-note underlay is correctly not invented. | P3 | lyric bindings | Require an authoritative underlay contract before exporting lyric syllables. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The guide follows the first melody/vocal part and exact tempo map, making entry and rough pitch audible. | P3 | guide selection | Add an explicit part selector when multipart rehearsal guides are needed. |
| 2 | The band-unlimited square wave is deterministic but can click and alias; documentation avoids quality claims. | P3 | guide waveform | Keep it diagnostic or add a separately versioned, deterministic softened profile. |
| 3 | WAV validation proves format and duration, not loudness, clipping perception, mix usefulness, or musical performance. | P3 | guide comparison | Require listening and audio-quality review before any candidate delivery. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Rendering is fully local and contains no provider, model download, network, or third-party egress path. | P3 | plan/render CLI | Preserve denied-network behavior for future notation adapters. |
| 2 | The receipt retains private hashes and filenames and is therefore correctly marked `shareable: false`. | P3 | receipt | Design a separate redacted projection only when an external exchange requires it. |
| 3 | Technical verification is explicitly separated from listening review, creative selection, rights, approval, delivery, and publication. | P3 | receipt limitations | Continue recording actual human decisions outside simulated role artifacts. |

## Synthesis

Roles reviewed: 4  
P1 blockers: 0 | P2 issues: 0 | P3 notes: 12

Verdict: APPROVED-WITH-CONDITIONS

Top finding: the one-measure MusicXML is structurally editable and round-trip
safe, but it is not engraved or session-ready notation.

Cross-role consensus: deterministic structural preservation is verified, while
musical usefulness, listening quality, creative fidelity, rights, and human
approval remain separate downstream gates.

## Amend

1. Before session use, add real bar layout, pickups, ties, and import checks in
   multiple notation applications.
2. Before vocal notation, add an authority-bound syllable-to-note underlay
   contract; do not infer underlay from lyric text.
3. Before delivery, add human score and sound review and a separately governed
   redacted exchange receipt if external sharing is required.

These are simulated role findings, not opinions or approvals from human
reviewers.
