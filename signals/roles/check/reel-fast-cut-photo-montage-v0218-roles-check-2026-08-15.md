---
skill: roles-check
topic: reel-fast-cut-photo-montage-v0218
date: 2026-08-15
roles_used: 5
p1_count: 0
verdict: APPROVED
---

# REEL fast-cut photo montage v0.2.18 — roles check

## Artifact

Code, tests, and documentation for the opt-in `montage` edit mode, hard-cut
assembly, `punch-in` / `punch-out` treatments, crop safety, and artifact
lineage. This is a CLI/renderer feature; `reel.manifest.v0.2` is unchanged.

## Selected roles

- Editor — cut language, cadence, and mode semantics.
- Animation Director — motion treatment and crop feasibility.
- Platform and Audience — phone-first montage behavior and compatibility.
- Sound Designer — visual timing contract with unchanged master-audio behavior.
- Story Director — whether technique remains subordinate to story and truth.

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Montage is explicit and defaults remain cinematic. | P3 | CLI `edit_mode` | Retain the compatibility regression. |
| 2 | Zero-duration assembly now uses concat instead of a zero-second xfade. | P3 | `still_animatic::render` | Keep hard-cut behavior covered at command and real-render levels. |
| 3 | Sub-second shots remain authored as ordinary conformed shots. | P3 | v0.2 contract | Evaluate pacing per work; do not invent a universal beat length. |

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Punch treatments create a materially distinct 20 percent scale range. | P3 | motion filters | Keep the value explicit and deterministic across backends. |
| 2 | Safety sampling uses the deeper crop for both punch directions. | P3 | `sampled_rects` | Preserve focal-point and protected-region gating. |
| 3 | The feature does not pretend to implement multi-source composites. | P3 | v0.2.18 docs | Add layouts later as a separate typed feature with its own safety model. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Montage is compatible with vertical delivery and caption-safe geometry. | P3 | real 720x1280 proof | Continue testing portrait exports, not only landscape plans. |
| 2 | Default BERTICA behavior remains a 0.8-second cinematic crossfade. | P3 | CLI regression | Treat any future default change as a breaking decision. |
| 3 | The release documents that montage overrides transition duration. | P3 | feature guide | Keep effective assembly and timing visible in artifacts. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Edit mode changes visuals only; the master-audio interface is stable. | P3 | render options | Let consumer manifests align music beats through conformed shot timing. |
| 2 | The real acceptance proof is intentionally silent and platform-valid. | P3 | vertical fixture | Exercise a scored montage in the Karts consumer package. |
| 3 | Source-audio mixing is not silently added to the still renderer. | P3 | feature boundary | Design video trims and source audio as a separate compositor contract. |

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Mode describes editorial grammar rather than asserting story facts. | P3 | CLI/docs | Keep provenance and truth boundaries in the consuming work. |
| 2 | Per-shot opt-in punch settings prevent indiscriminate motion. | P3 | manifest usage | Use punch beats for emphasis, not every shot. |
| 3 | Existing narrative cuts are not rewritten by the release. | P3 | compatibility | Introduce montage as a derivative in Karts, preserving the liked master. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 0 | P3 notes: 15

Verdict: **APPROVED**

Top finding: montage is an auditable opt-in mode and does not change BERTICA's
cinematic default.

Cross-role consensus: the feature boundary is appropriately narrow; validate
the energetic result in the consuming Karts work rather than encoding one
universal montage rhythm in Reel.

## Amendments

1. Add a Karts montage derivative using real sourced photographs and the new
   mode, while preserving the current action proof.
2. Add multi-source layouts only through a future typed contract with focal,
   caption, provenance, and mobile-safe composition checks.
3. Add video trim and source-audio support as a separate compositor capability,
   not an implicit extension of still-image animatic rendering.

## Evidence

- `cargo test --all`: 77 library tests and all active integration suites passed.
- Real render: 720x1280, H.264/yuv420p, 24 fps, 6000 ms, silent, captioned.
- `animatic-check`: passed with four verified inputs and seven render
  capabilities.
- Real proof SHA-256:
  `3cbe6e1904e28145bd667434fc5425c0da9ac9e0667d224f25c7224e48e8686d`.
