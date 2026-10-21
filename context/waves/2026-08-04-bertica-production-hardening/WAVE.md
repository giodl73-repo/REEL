# Wave: BERTICA-driven production hardening

## Goal

Turn repeated BERTICA animatic production costs into portable REEL v0.2
contracts and deterministic CLI transformations without copying private assets
or creating a consumer runtime dependency on REEL internals.

## Pulses

| Pulse | Title | Status | Outcome |
|---:|---|---|---|
| 01 | Production contract and conform | complete | Add untimed planning, speaker cues, protected pauses, atomic conform, source coverage, privacy egress, lineage, quality checks, migration, and sanitized acceptance fixtures. |
| 02 | Asset-backed animatic adapter | complete | Add deterministic FFmpeg still-animatic rendering and provenance-rich artifact reports. |
| 03 | Hardening and adoption | complete | Added CI/release/install guidance, completed 12/12 read-only BERTICA migration validation, rendered a 7.500-second Windows/WSL FFmpeg proof with A/B variants, and documented the consumer handoff. |
| 04 | Executable voice performance | complete | Added exact-span performance direction, Chatterbox/generic compilation, executed-versus-advisory disclosure, atomic path-free receipts, re-verification, sanitized fixtures and v0.2.15 CLI tests. |
| 05 | Scoped cadence evidence | complete | Separated emotion scope, register, contour, terminal boundary, relative pitch and joins; added explicit IndexTTS 2.5 capability disclosure plus path-free, hash-bound prosody evidence that detects a requested fall rendered as a rise. |
| 06 | Music reconstruction foundation | complete | Added the provider-neutral `reel-music` crate, immutable raw-PCM and exact timebase contracts, neutral identity proof, typed repair plans with complete changed/locked coverage, specialist roles, synthetic fixtures, and Windows/Linux validation. |

## Invariants

### 2026-10-20 bounded version controls and read-only legacy protections

Pulse 09 adds explicit version-control base/patch compaction and compatible
legacy protection exports. See docs/asset-version-index-v2.md. No consumer
checkout is modified by reading its protections; actual apply keeps its gates.

### 2026-10-20 verified redundant-copy removal

Pulse 08 adds an explicit guarded Windows hot-copy removal command, immutable
journals and recovery. See docs/asset-demotion-v1.md. Active admissions and
unsupported consumer metadata hold maintenance; no archive/last-copy retirement
is introduced. Private production pilot evidence stays upstream.

### 2026-10-20 advisory configurable tier planning

Pulse 07 adds read-only baseline/delta tier proposals and metadata freshness
checks in reel-assets. See docs/asset-tiering-v1.md. There is no apply or deletion
API in this slice; private consumer inventory remains upstream.

### 2026-10-20 project-configured asset custody and accounting

Extracted the shared local custody/version/LFS/job implementation into the
`reel-assets` crate, with synthetic compatibility fixtures and project-configured
read-only accounting. Private consumer data remains upstream. See
`docs/asset-project-v1.md` and the matching dated handoff for scope and validation.
The report never authorizes deletion, archive probing or creative selection.

### 2026-09-09 episode consumption and comparative review

Added exact decoded scene-to-episode picture/audio verification, D/M/E
recombination, continuous timestamp checks, explicit external-layer dispositions
and cut-boundary review findings. Intentional boundaries bind exact scene receipts.
Added native scene A/B review with identical dialogue and three sound variants,
reusing the existing audio-quality gate. See `docs/episode-delivery-v0.1.md` and
the matching review/handoff for measured scope and remaining creative checks.

### 2026-09-09 compiled scene delivery

Added the scene-delivery plan/render/check bridge from the cue-relative compiler
to exact sample-addressed D/M/E and globally partitioned picture frames. The
contract enforces composition limits and explicit missing/silent bus choices,
retains FFV1/PCM masters, and separately encodes review media. The existing VFX,
caption and changed-only tools retain ownership of their specialized operations.
See `docs/scene-delivery-v0.1.md`, `docs/reviews/2026-09-09-scene-delivery.md`, and
`docs/handoff/2026-09-09-scene-delivery.md` for adoption limits and evidence.

- REEL never uploads or copies BERTICA private photographs, voice, manuscript
  text, or binary renders.
- Transformations create derivatives and do not overwrite planning sources.
- Human approval is referenced, never inferred.
- Child implementation commits precede TRACKER pointer updates.

## Validation

### 2026-10-21 generic asset-manager review workflow

Added explicit inventory refresh with immutable private review inputs, generic
project templates and configured local root policy. The offline UI supports tier
filters, size sorting, chosen-block inspection, hold reasons and bounded hash
previews. Native dry-run/application retains ownership of cache changes. See
`docs/handoff/2026-10-21-generic-asset-manager-ui.md` for validation and adoption
limits. Private assets and reports remain in the producing repository/cache.

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run --quiet -- validate manifests/fixtures/two-speaker-untimed/planning.yaml --output json
git diff --check
```
