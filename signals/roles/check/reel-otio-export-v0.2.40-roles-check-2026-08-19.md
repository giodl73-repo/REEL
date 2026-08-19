---
skill: faces-development-loop
topic: reel-otio-export-v0.2.40
date: 2026-08-19
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.40 OTIO export roles check

## Frame

**Working owner system:** REEL already validates ordered, gap-free production
shots at millisecond precision. BERTICA's conformed S1E01 manifest contains 70
shots over 26:09.409 and remains authoritative for story scope, timing, asset
status, and review state.

**Missing shared capability:** editorial tools cannot consume that timeline
without a hand-maintained translation.

**Thesis:** REEL can export one standards-shaped OpenTimelineIO timeline that
preserves exact shot identity and timing without becoming an NLE, leaking local
media paths, or inventing transition and approval semantics.

**Deletion target:** BERTICA can retire one manually maintained shot-timing
translation and import or inspect the REEL-generated `.otio` timeline instead.

**Disproof:** the slice fails if S1E01 timing changes after round-trip through
the official OTIO reader, if local paths or prompt prose enter the output, or if
REEL must guess edit transitions, selected media, handles, frame rate, or
creative authority.

## Audit

- `production::validate` already rejects duplicate identities, unknown scene
  links, missing or zero timing, non-contiguous shot starts, mismatched scene
  totals, and platform/export duration mismatch.
- REEL normalizes authored seconds to integer milliseconds. BERTICA uses
  millisecond durations that are not consistently aligned to 24, 25, or 30 fps.
- `transition_out` is free-text creative intent, not a duration-bearing
  transition contract.
- `visual_asset` is a consumer-local path and `visual_asset_status` distinguishes
  planned, candidate, selected, approved, and missing media.
- BERTICA's current S1E01 has five candidate assets and sixty-five planned
  assets; none is selected.

## Comparison

### Internal analogues

| Analogue | Decision | Reason |
| --- | --- | --- |
| Production validation and millisecond conform | Reuse | It is already the timing authority and rejects gaps or overlap. |
| Path-free provider and economics reports | Reuse | Interchange should carry hashes and normalized facts, not workstation paths. |
| Asset readiness and promotion | Reuse | Candidate, selected, and approved remain distinct and must survive as metadata. |
| FFmpeg render planning | Avoid | Render readiness and media decoding are not required for an offline editorial timeline. |
| Free-text `transition_out` | Adapt | Preserve an exact hash binding in metadata; do not copy prose or manufacture OTIO `Transition` offsets. |

### External comparators

OpenTimelineIO's official file-format specification defines a `Timeline`
containing a `Stack`, `Track`, and ordered `Clip` children, with exact
`RationalTime`/`TimeRange` values and namespaced metadata. It recommends the
`.otio` extension and explicitly supports `MissingReference` for offline media:

- <https://github.com/AcademySoftwareFoundation/OpenTimelineIO/blob/main/docs/tutorials/otio-file-format-specification.md>
- <https://github.com/AcademySoftwareFoundation/OpenTimelineIO/blob/main/tests/baselines/empty_clip.json>
- <https://github.com/AcademySoftwareFoundation/OpenTimelineIO/blob/main/tests/baselines/empty_missingreference.json>

Useful precedent: exact rational time, offline references, and namespaced
metadata. Negative precedent: adapter availability varies and may lose
transitions or metadata, so V1 emits native OTIO only and makes no claim about
AAF, EDL, FCP XML, Premiere, Resolve, or media relinking behavior.

## Role findings

### Editor

- **P2:** Millisecond timing must not be rounded to an invented frame rate.
  Export at rate 1000 and record that this is a timing timebase, not delivery
  fps.
- **P2:** Free-text cut intent cannot become a dissolve or wipe without
  duration and handle semantics. Preserve only its exact SHA-256 binding under
  `metadata.reel`.
- **P2:** Export only conformed or locked timelines; guide and untimed plans are
  too provisional for the deletion target.

### Animation Director

- **P2:** Candidate art must not become linked or selected media. Emit
  `MissingReference` for every V1 clip and preserve media kind and asset status.
- **P3:** Motion and camera direction are useful editorial context but remain
  REEL metadata, not OTIO effects.

### Platform and Audience

- **P2:** One portable video track is the smallest interoperable unit. Do not
  claim delivery fitness or adapter compatibility.
- **P3:** Aspect ratios and export targets remain in the owner manifest; V1
  need not duplicate them to prove editorial interchange.

### Story Director

- **P2:** Preserve stable work, scene, and shot identity plus the exact source
  manifest hash. Do not export prompts, action prose, continuity notes, source
  manuscript paths, or private rationale.
- **P2:** Editorial import cannot change canon or imply creative approval.

### Sound Designer

- **P2:** A picture-track-only export must say so explicitly. Do not flatten
  narration, score, ambience, and effects into one speculative audio track.
- **P3:** Audio OTIO tracks require separate source ranges, media references,
  overlaps, fades, and mix authority and are deferred.

## Security and simplicity conditions

1. V1 accepts only a validated `reel.manifest.v0.2` with `conformed` or `locked`
   timing.
2. Every clip uses exact integer millisecond `RationalTime` values at rate 1000.
3. Every clip uses `MissingReference`; no local paths, URLs, prompts, or prose
   descriptions are emitted.
4. `metadata.reel` carries only bounded portable identity and normalized
   technical facts.
5. Output is no-clobber and reports the exact source manifest SHA-256.
6. One video track only; no transitions, audio tracks, markers, effects,
   alternates, media embedding, adapters, import, or round-trip mutation.
7. Export never implies selection, creative approval, rights approval,
   publication, or release.

## Verdict

**APPROVED-WITH-CONDITIONS.** The slice is smaller than a general OTIO adapter:
it is a deterministic offline picture-timeline export. The required proof is
the real BERTICA S1E01 manifest parsed by the official OTIO implementation with
70 ordered clips and exactly 1,569,409 milliseconds of duration.
