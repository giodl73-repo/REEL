---
skill: roles-check
topic: music-arrangement-candidate-c12
date: 2026-09-01
roles_used: 6
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# C12 arrangement-candidate role review

This is a simulated checklist review, not the opinion or approval of any named
human authority. It cannot establish listening, recognition, selection, rights,
or release.

## Artifact identification and selection

Artifact type: schema, validator, CLI, tests, and user-facing contract. Selected
roles are Music Reconstruction Engineer (lineage/round trip), Score and
Arrangement Director (musical inheritance), Sound Designer (audible evidence),
Editor (comparison order), Rights and Provenance Steward (private lineage and
egress), and Platform and Audience (candidate delivery boundary).

## Findings

### Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Plan, model, export plan, receipt, and audio are independently hash-bound | P3 | validator | Retain recursive checks |
| 2 | MIDI and MusicXML use the existing independent round-trip checker | P3 | score export | Keep both gates mandatory |
| 3 | The audible proof is explicitly a rehearsal guide, not a master | P3 | documentation | Preserve limitation in later schemas |

### Score and Arrangement Director

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Every mapped note is materialized once in its declared instrument part | P3 | model inheritance | Retain exact comparison |
| 2 | Non-note musical identity cannot drift in v0.1 | P3 | scope | Add governed transformation only in a versioned successor |
| 3 | Recognition remains a human decision after listening | P3 | gates | Never infer it from structural equality |

### Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | The score has an audible byte-bound derivative | P3 | audio binding | Keep guide separate from performance master |
| 2 | Mix balance is present in the listening rubric | P3 | comparison | Retain in future candidate types |
| 3 | No validator path renders or auditions audio | P3 | non-execution | Keep checking read-only |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Listening precedes recognition and selection | P3 | gate order | Preserve causal order |
| 2 | Blind labels are distinct | P3 | comparison | Avoid identity leakage in review packages |
| 3 | Rejection is recorded rather than silently discarded | P3 | selection | Preserve rejected candidates and reasons upstream |

### Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Local-only creation forbids an unnecessary egress decision | P3 | provenance | Retain explicit network policy |
| 2 | External creation requires an immutable decision | P3 | provenance | Add adapter license fields before generative audio support |
| 3 | The report remains private and contains no consumer fixture | P3 | privacy | Keep project-specific receipts outside REEL |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---:|---|---|---|---|
| 1 | Candidate delivery is explicitly private | P3 | report | Add delivery formats only in a separate contract |
| 2 | The eight-lens comparison covers audible audience recognition | P3 | comparison | Test phone/listening context upstream when relevant |
| 3 | No publication state is inferred from selection | P3 | boundary | Keep release as a separate project gate |

## Synthesis

Roles reviewed: 6

P1 blockers: 0 | P2 issues: 0 | P3 notes: 18

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: structural and audible round trips are evidence, not human musical
recognition. Cross-role consensus: keep the rehearsal guide, performance
master, selection, and release as separate artifacts and gates.

## Amendments

1. Document the v0.1 preserved-non-note limitation and guide-only audio scope.
2. Require platform/audience review for private candidate delivery.
3. Keep all reports private and all human states decision-backed.

All three amendments are present in the implementation and documentation.
