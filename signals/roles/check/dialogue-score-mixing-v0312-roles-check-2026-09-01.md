---
skill: roles-check
topic: dialogue-score-mixing-v0312
date: 2026-09-01
roles_used: 6
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# Dialogue-score mixing role review

This is a simulated checklist review, not an actual person's opinion or
approval. It cannot select music, approve balance, establish a Golden, or
authorize release.

## Artifact identification and selection

Artifact type: manifest schema, deterministic FFmpeg compiler, stem renderer,
quality evidence, receipts, CLI, tests, and documentation. Selected roles are
Sound Designer, Editor, Music Reconstruction Engineer, Score and Arrangement
Director, Rights and Provenance Steward, and Platform and Audience.

## Findings

### Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Speech detectors and ducking targets are explicit role buses | P3 | routing | Preserve disjoint detector/target validation |
| 2 | The dry-floor blend bounds reduction without muting score | P3 | ducking | Retain synthetic floor assertion |
| 3 | D/M/E are post-duck and pre-master, so recombination reflects the audible premaster | P3 | stems | Keep mastering semantics in receipts |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Beat and local-time automation resolve to one ordered graph | P3 | automation | Keep strict duplicate/out-of-range failures |
| 2 | Hold, linear, and smooth segment ownership is documented | P3 | interpolation | Preserve left-point curve semantics |
| 3 | No-score and speech-readable variants require no manifest edit | P3 | review | Keep variants mechanically derived |

### Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Source, manifest, policy, tool, receipt, and output hashes are independently bound | P3 | lineage | Recheck all bindings before reuse |
| 2 | All core outputs have exact sample geometry | P3 | WAV contract | Keep PCM parser and sample-count checks |
| 3 | Quantized D+M+E recombination has a documented three-LSB bound | P3 | evidence | Do not replace it with duration-only evidence |

### Score and Arrangement Director

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | The grammar expresses phrase-level moves without choosing cues | P3 | authority | Keep cue choice outside REEL |
| 2 | Music-only targeting leaves effects and ambience unchanged by default | P3 | mix intent | Require explicit targeting for any exception |
| 3 | Dynamic-EQ intent is complete but render support is honestly gated | P3 | P1 scope | Add a portable implementation only with cross-platform proof |

### Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | The shareable stem receipt has hashes and basenames but no local paths | P3 | privacy | Keep path-bearing execution data in the private artifact report |
| 2 | No external service, upload, synthesis, or project asset is introduced | P3 | egress | Preserve local-only execution |
| 3 | Receipts state evidence, not creative or release approval | P3 | authority | Keep human approval in project decision records |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Mono and small-speaker variants expose translation risk | P3 | review variants | Keep them review proxies, not delivery masters |
| 2 | Dialogue loudness and margin targets are manifest policy, not REEL defaults | P3 | delivery evidence | Require owner-authored targets |
| 3 | Failed measurements remain visible without inferring rejection | P3 | review state | Preserve evidence/approval separation |

## Synthesis

Roles reviewed: 6

P1 blockers: 0 | P2 issues: 0 | P3 notes: 18

Verdict: **APPROVED-WITH-CONDITIONS**

Conditions: keep dynamic EQ plan-only until portable FFmpeg behavior is proven;
retain path-free stem receipts and separate private execution reports; do not
turn technical evidence into creative approval. All conditions are explicit in
the v0.3.12 implementation and documentation.
