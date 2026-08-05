---
work: bertica-cross-repo-response
stage: handoff
format: short-film
style: illustrated-2d
source_scenario: C:/src/bertica
author: reel-production
rubric_version: v0.1
created: 2026-08-04
updated: 2026-08-04
sources:
  - docs/handoff/2026-08-04-bertica-production-requests.md
  - docs/production-manifest-v0.2.md
---

# REEL response to BERTICA production requests

REEL accepted the complete request as the basis for production manifest v0.2.
BERTICA continues to own manuscript truth, voice consent, private photographs,
character facts, and publication approval.

## Implemented request mapping

| Request | REEL v0.2 response |
|---|---|
| Untimed pre-voice manifests | `timing_status`, optional planning timing, untimed `validate`/`plan`, and precise render gates. |
| Audio conform and atomic retiming | `conform`, measured cue files, per-speaker tempo, protected pauses, atomic packets, and hash lineage. |
| Speaker-aware narration | Stable `speakers`, `narration_cues`, approval references, asset kind, text/source/shot identity, and pause policy. |
| Source/omission provenance | `source_ranges`, cue/shot `source_refs`, explicit omissions/bridges, and `source-coverage`. |
| Privacy-safe continuity | Textual entity observations, local reference hashes/policies, and blocking path-free `provider-package`. |
| Variant lineage/review | Parent/root/reason/dimensions/candidate/approval fields, separate principal findings, and `review-select`. |
| Long-still controls | Hold, focal point, protected region, depth, screen direction, eye-line, no-lip-sync, and A/B declaration checks. |
| Production renderer | Asset-backed `animatic-render` with motion, dissolves, captions, disclosure, input/output hashes, and dry-run support. |

## BERTICA consumption contract

BERTICA should consume the conform packet's `manifest.yaml`, `captions.srt`, and
`lineage.json`. It can render with REEL's `animatic-render` executable or retain
its local renderer while both consume the same conformed manifest.

No BERTICA production manifest, manuscript text, photograph, voice, or binary
render was copied. REEL instead added the synthetic
`manifests/fixtures/two-speaker-untimed/` acceptance fixture.

Existing manifests migrate into new derivative files with `reel migrate`.
Legacy timing can be millisecond-normalized, and legacy shot narration is lifted
into an explicitly review-required `legacy-narrator` cue rather than silently
claiming speaker identity or approval.

For the Mayabeque planning sequence, BERTICA's exact consumption shape is:

```powershell
reel validate production/reel/mayabeque-planning.yaml --output json
reel plan production/reel/mayabeque-planning.yaml --output json
reel conform production/reel/mayabeque-planning.yaml `
  --cues production/audio/mayabeque-cue-measurements.yaml `
  --speaker-tempo bertica=85 `
  --output-dir production/reel/conformed/mayabeque-v1
reel source-coverage production/reel/conformed/mayabeque-v1/manifest.yaml --output json
reel quality-check production/reel/conformed/mayabeque-v1/manifest.yaml --output json
```

The renderer consumes
`production/reel/conformed/mayabeque-v1/manifest.yaml` and
`captions.srt`; review/provenance automation consumes `lineage.json` and
`conform-report.json`. Paths are illustrative until BERTICA creates the
Mayabeque planning and cue-measurement artifacts.
