---
skill: roles-check
topic: reel-music-slice-b-v027
date: 2026-09-01
roles_used: [music-reconstruction-engineer, sound-designer, editor, rights-provenance-steward]
p1_count: 0
verdict: APPROVED
---

# Roles check: `reel-music` Slice B v0.2.27

## Artifact and role selection

Artifact: Rust EDL/evidence contracts, FFmpeg raw-PCM renderer, CLI commands,
generated tests, and v0.2.27 documentation. The required smallest panel from
`.roles/ROLE.md` is reconstruction engineering, sound design, editing, and
rights/provenance. Score, lyric, story, animation, and audience roles are not
selected because this slice creates no score, text adaptation, picture,
narrative artifact, or delivery package.

## Music Reconstruction Engineer

| # | Finding | Severity | Evidence | Recommendation |
|---|---|---|---|---|
| 1 | The EDL is recompiled from current repair/source lineage and resolves edits entirely in integer sample coordinates. | P2 resolved | `edl.rs` | Preserve sample identity when container decoding is added. |
| 2 | Every changed envelope equals one executable cut, while every outside sample maps to an exact keep segment. | P2 resolved | EDL validation and mutation tests | Keep this proof mandatory for future grouped operations. |
| 3 | The renderer uses explicit raw format/rate/channels and sample-index `atrim`; real FFmpeg output equals the expected byte concatenation on Windows and Linux. | P2 resolved | `music_render.rs`, external test | Retain adapter-version evidence and cross-platform test vectors. |
| 4 | Non-cut operations remain planning-only rather than gaining guessed execution semantics. | P3 | compile rejection test | Specify canonical grouping and composition before enabling crossfade/replace. |

## Sound Designer

| # | Finding | Severity | Evidence | Recommendation |
|---|---|---|---|---|
| 1 | Seam evidence measures boundary delta, RMS balance, waveform-window correlation, spectral distance, and DC-offset difference. | P2 resolved | `evidence.rs` | Calibrate thresholds with private listening evidence before production defaults evolve. |
| 2 | The right retained segment must remain exact and meet a minimum tail length. | P2 resolved | join evidence and test | Add ambience-specific and decay-envelope evidence when crossfades become executable. |
| 3 | Documentation correctly avoids calling cosine window correlation true phase reconstruction or an inaudibility proof. | P2 resolved | v0.2.27 contract | Keep metric labels narrower than the perceptual claims they can support. |
| 4 | The initial thresholds are engineering defaults tested on a periodic synthetic fixture, not broad musical validation. | P3 | strict policy v0.1 | Version threshold profiles and preserve human listening as a separate gate. |

## Editor

| # | Finding | Severity | Evidence | Recommendation |
|---|---|---|---|---|
| 1 | Ordered half-open cuts remove ambiguity and compile into explicit source/output segment mappings and join locations. | P2 resolved | `EditDecisionList` | Preserve canonical ordering in future operation groups. |
| 2 | Cuts at either signal edge are rejected because this seam profile requires evidence on both sides. | P2 resolved | EDL compiler | Add a separately specified edge-trim profile if editorial need arises. |
| 3 | Candidate duration is derived from retained samples and independently enforced through byte length. | P2 resolved | compile/evidence tests | Surface duration deltas in later comparison packages. |
| 4 | Cut-only execution is intentionally conservative and cannot yet express a motivated crossfade or replacement. | P3 | v0.2.27 scope | Add those only with deterministic overlap/order semantics and review evidence. |

## Rights and Provenance Steward

| # | Finding | Severity | Evidence | Recommendation |
|---|---|---|---|---|
| 1 | EDL and evidence bind repair, source, decoded signal, candidate, and adapter version without modifying the source. | P2 resolved | contract hashes and source path handling | Keep source directories read-only in consuming projects. |
| 2 | FFmpeg execution is local, adds no network/model dependency, and existing outputs are never overwritten. | P2 resolved | root adapter and preflight | Retain network-denied project policy around private assets. |
| 3 | The evidence artifact is path-free but conservatively marked non-shareable because lineage hashes may still identify private media. | P2 resolved | `EvidenceReport` | Design a separately reviewed redacted exchange receipt if needed. |
| 4 | Technical pass, human listening, selection, delivery, and release remain distinct; simulated role findings are not approval. | P2 resolved | CLI/docs/review language | Record actual decisions against exact artifact hashes outside REEL. |

## Synthesis

Roles reviewed: 4
P1 blockers: 0 | P2 issues: 0 open (13 resolved/passing) | P3 notes: 3

Verdict: **APPROVED**

Consensus: v0.2.27 is a defensible cut-only execution slice. It proves exact
unaffected signal and deterministic seam/tail metrics without claiming a
creative repair is acceptable. The next executable operation must receive its
own composition semantics and evidence profile rather than bypassing this
boundary.

Human approval recorded: **none**.
