---
skill: roles-check
topic: music-interchange-c3
date: 2026-09-01
roles_used: 4
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL role review: existing-tool music interchange C3

## Artifact identification

- Type: Rust contract validator, CLI, sanitized fixtures, tests, documentation,
  and public-evidence inventory.
- Domain: source decomposition, transcription, feature annotations, notation,
  private media, provenance, and interoperability.

## Role selection

- Music Reconstruction Engineer: source/container/PCM identity and analyzer
  uncertainty.
- Sound Designer: stem and sonification boundaries and listening-quality claims.
- Editor: ordering and selection of competing evidence entering production.
- Rights and Provenance Steward: software/model/dataset identity, licenses,
  network denial, and private receipt state.

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Source, external container, and normalized PCM identities remain separate and exact. | P3 | intake validator | Preserve this three-layer identity when semantic import is added. |
| 2 | Format sniffing detects ordinary stale/false declarations but is intentionally not a complete parser for every upstream format. | P3 | signature checks | Add profile-specific parsers only from sanitized real-user fixtures. |
| 3 | Normalized PCM is hash-bound but the validator does not rerun the declared decoder. | P3 | normalized stem | Add a deterministic decode receipt/check command if operators need reproducible conversion proof. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Stem estimates and sonifications are distinct purposes even when both use WAV/FLAC. | P3 | purpose matrix | Keep them distinct in all later audition and mix workflows. |
| 2 | Semantic roles remain user-declared strings, preserving existing tool labels without asserting their accuracy. | P3 | artifact roles | Map roles explicitly into typed analysis stem roles during the next conversion step. |
| 3 | Intake measures no bleed, consistency, clipping, loudness, alignment, or listening usefulness. | P3 | capability boundary | Require analysis and listening evidence before any stem enters repair or mix decisions. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Multiple competing outputs can coexist without one being silently selected. | P3 | artifacts list | Add an explicit comparison/selection queue rather than relying on manifest order. |
| 2 | Intake does not infer temporal event order from CSV/JAMS/MIDI, avoiding accidental edit authority. | P3 | semantic boundary | Convert timestamps only through a typed, source-timebase-aware adapter. |
| 3 | Validation does not decide which evidence should shape form, pacing, cuts, or corrections. | P3 | review boundary | Preserve editor findings separately when actual candidate evidence is compared. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Producer identity covers executable hash, parameters, software license, dataset disclosure, and optional model hash/license. | P3 | producer contract | Preserve exact installed-artifact hashes in operator-generated manifests. |
| 2 | Producer and decoder network policies are denied, and the CLI initiates no tool or service execution. | P3 | local policy | Keep any future execution command separate from intake validation. |
| 3 | Source/artifact hashes make the report private and it is correctly marked non-shareable. | P3 | report | Create a separate redacted projection only for an explicitly approved exchange. |

## Synthesis

Roles reviewed: 4  
P1 blockers: 0 | P2 issues: 0 | P3 notes: 12

Verdict: APPROVED-WITH-CONDITIONS

Top finding: format-shape validation is an appropriate first compatibility
boundary, but real semantic conversion must be driven by sanitized operator
fixtures and typed timebase mappings.

Cross-role consensus: intake identity is technically sound; accuracy, quality,
selection, correction, rights, and approval remain downstream human-governed
steps.

## Amend

1. Acquire sanitized outputs from actual operator workflows before adding any
   profile-specific semantic importer.
2. Add deterministic decode re-execution evidence only when a real workflow
   requires REEL to prove container-to-PCM conversion.
3. Add a comparison and selection queue before allowing competing imported
   evidence to influence a corrected model.

These are simulated role findings, not actual human opinions or approvals.
