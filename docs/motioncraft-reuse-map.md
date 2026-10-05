# Motioncraft implementation map

## Authoritative baseline

Implementation branch: `codex/motioncraft`, based on current REEL main
`4effab1`. The original chat checkout at `C:/src/REEL` is older (`3d42302`)
and does not contain the current scene engine. Implementation and verification
take place in the managed Motioncraft worktree, not the old checkout.

## Reuse and gaps

| Existing seam | Reuse | Missing capability |
|---|---|---|
| `reel-assembly::scene_authoring::Episode/Scene` | Scoped IDs, language-local cues, selected asset bindings, authoring fingerprints | Optional inherited episode/scene/shot motion direction; include intent in fingerprints |
| `scene_delivery_compile` | Exact native event spans, graph and pointer validation, media bindings | Resolve inherited intent, bind compiled motion evidence to the immutable selected delivery input |
| `scene_delivery::PictureMotion` | Still-picture render transform, exact delivery frame allocation | Authored phased motion, explicit reading holds, reduced-motion behavior |
| `scene_delivery::render/check` | Actual scene execution, selected overlays, no-clobber receipts | Consume phased plans; extract indexed samples and verify phase/hold evidence |
| `reel-scene-build` | Authoring-to-delivery identity checks, scene execution and receipt | Verify intent is retained rather than dropped between authoring and delivery |
| Production packages | Hash-pinned inventory and review evidence | Include intent, compiled plan, and review artifacts in supported component kinds |
| CAIMITOS adapter/cache boundary | Stable source IDs, selected graph revisions, sample clocks, verified hydration | New-authoring/upgrade integration and a synthetic motion canary |

Motion intent must participate in selected authoring identity. A detached
sidecar that does not reach the scene renderer cannot satisfy this project.

## First implemented core

`reel-assembly::motioncraft` defines strict direction/element/phase inputs,
explicit normalized geometry, bounded zoom curves, optional phases, whole-
direction override precedence, native-duration alignment residuals, and
deduplicated boundary/hold/contact-sheet review samples. The subsequent
checkpoint connects it to scoped episode/scene authoring, fingerprinting,
native scene-delivery compilation, actual phased camera rendering and indexed
evidence. Typography execution and CAIMITOS adapter adoption remain open.

Phase frames are zero-based inclusive working frames. Between phases the last
zoom is retained; adjacent declared phases must have continuous zoom values.
Working spans may differ from the exact native duration by at most half a
working frame; the residual is exposed rather than changing the sample clock.
The renderer must distinguish element holds from whole-picture holds.

## Remaining work

1. Verify owner-rendered visual intent in the retained comparison. Typed
   visual/profile direction, explicit native-duration fitting and transformed
   protected-region validation are implemented and covered by focused tests.
2. Audit immutable intent/selected-revision binding across all consumer entry
   points; scene-build already rejects direction dropped from its selected job.
3. Complete playback-based scored review of the six retained queue renders.
5. Establish the CAIMITOS integration lane, implement adapter adoption,
   upgrade/rollback, hydration, and canary assembly checks.
6. Complete final role evaluation and the requirement-by-requirement audit.

CAIMITOS lane diagnostic: `production-engineering` is absent from the current
registry; `animation-vfx` is `requires-dedicated-worktree`. The session-handoff
skill requires administration to establish that lane before production writes.
REEL implementation is independent of that pending lane assignment.
