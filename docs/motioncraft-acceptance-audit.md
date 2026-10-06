# Motioncraft final technical acceptance — 2026-10-05

The reviewed implementation goal passes across REEL and the actual CAIMITOS consumer. Historical checkpoint findings are retained in `motioncraft-acceptance-checkpoint-history.md`. Human creative selection, publication and main-branch integration remain separate from technical acceptance.

| Requirement | Final evidence and disposition |
|---|---|
| Strict authored phases, purpose, focal element, protected regions, visual brief, reduced motion | Shared direction compiler tests pass. Episode/scene/shot inheritance and failure behavior are exercised through the actual CAIMITOS binary. |
| Native clocks and selected media | The consumer verifies selected cache bytes before output. Hash-bound immutable overlays retain authoring, selected takes, cue samples and effect placements. |
| Native camera geometry and holds | Actual consumer testing exposed FFmpeg's one-based frame counter. REEL afc4612fa3b2bc1f662f1eac511157c331883aab subtracts one. The short-camera regression fails before and passes after; all 21 scene-delivery tests including FFmpeg pass without relaxed cadence thresholds. |
| Caption reservation/reduced motion | Both study caption profiles pass. Illustrated consumer variants have 96,000 native samples and 48 frames; every reserved band is clear, reduced-motion cues stationary, and audio identical. |
| Matched 15-second study | Eight corrected renders/cadence reports pass with identical assets, audio and 450 frames. Corrected boards and normal/quarter desktop/phone playback inspected. Advisory scores retained: landscape 69 to 72, portrait 70 to 73; Rhythm/Legibility rise, Emotion/Execution unchanged. No human viewer is claimed. |
| Exact package/producer proof | r4 has 834 components. Package SHA b2013f68af58f3a8fa07d0fca82c01d809136454740fe2a2bf396467668e3970; production receipt SHA 47a35e1608fe99fd70c3b59cf9081ce6c2a84839bff8458f279db41a63a1eff9. All four clean profiles reproduce. Historical packages remain intact and do not replace corrected camera proof. |
| Actual future/existing CAIMITOS adoption | Consumer branch codex/motioncraft-integration pins afc4612. All 25 Rust targets compile; selected compiler 8 pass/3 private tests ignored; editor 3, authoring 10, immutable-job 7 and cache test 1 pass. Registry and existing audio compatibility checks pass. |
| Mandatory illustrated odd-sample canary | Actual adapter/immutable upgrade/native engine: cues [48001,47999], effect [123,139), 48 frames, unchanged PCM, cadence and caption/reduced behavior pass. Illustration is original fictional engineering media. |
| Existing episode upgrade/rollback | Private S1E04 scene-001 baseline and upgrade: 636 frames, four exact stems, only first selected picture directed; all remaining 584 frames match. Source selection and original job retained. No creative acceptance inferred. |
| Canonical cache and clean consumer | 1,078-object producer register, hydration manifest and technical authority receipt pass. Fresh actual CAIMITOS compilation/rendering reproduces compiled bytes, 450 decoded frames and four native stems. No central Golden register changed. |
| Rejection and final roles | Unknown/stale/missing/drift inputs, unsupported easing/conflicting holds, overwrite and tampering reject. Final advisory role review is in signals/roles/check/reel-motioncraft-final-integration-2026-10-05.md. |

CAIMITOS commands, authority and independent checks are under `production/animation-vfx/motioncraft/README.md`. Its ready integration request at `projects/handoffs/motioncraft-caimitos-v1.json` passes against pushed checkpoint 5edd2ffcae17d5f3ca7740f9fc5276a3ba2add56. Both feature branches are reviewable. No main merge, publication, cast, Golden or episode-current promotion has been performed.
