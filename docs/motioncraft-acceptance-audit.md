# Motioncraft acceptance audit — incomplete checkpoint

Authority: reviewed v4 plan in `reel-motioncraft-plan-2026-10-05.md`, its six
deliverables and the user's requirement for actual CAIMITOS new/existing
episode use through the scene engine. This audit preserves that scope.
It does not mark the goal complete. Evidence below is current local code,
executed tests and retained runtime artifacts; prose describing intent is not
proof of adoption.

## Requirement evidence

| Requirement | Current evidence | Status / remaining proof |
|---|---|---|
| Reuse/gap map and documented opt-in contract | `motioncraft-reuse-map.md`, `motioncraft-authoring.md`; optional episode/scene direction fields in `reel-assembly` | Verified for bounded camera treatment; no hidden shape/type execution claimed. |
| Stable element/phase IDs, purpose/focal priority, optional phases, palette/type roles | Strict direction types and compiler tests; visual brief retained in actual review JSON | Verified. Art/type realization remains owner-rendered selected graphics, not automatic recoloring. |
| Episode → scene → language/shot precedence; legacy absence | 19 scene-authoring tests, conditional fingerprint projection; old jobs/inputs remain supported | Verified in REEL; actual consumer inheritance still unmet. |
| Native samples, inclusive working frames, delivery allocation and residuals | Core compiler tests; explicit fit test; 48,001/47,999 actual consumer baseline; 1/2000-frame native boundary residual | Verified without audio retiming. |
| Invalid spans, IDs, stale bindings, unsupported intent and duration mismatch reject | Core/authoring/delivery tests; scene-build rejects changed authoring, stale selected lock and edited job before output creation | Verified in REEL; repeat at actual consumer boundary. |
| Intentional concurrency is allowed; overlap samples are bounded/deduplicated | New two-element planning test checks overlap endpoints/interior and independent hold sample; moving phase budget 512 | Verified at compiler level. Current renderer explicitly rejects unsupported multi-element execution. |
| Explicit profile safe areas and transformed protected regions | Delivery profile field, endpoint containment tests, both study profile renders | Verified for declared camera geometry. No automatic subject detection inferred. |
| Reuse caption-band reservation | Corrected caption derivatives clear the inspected diagram label; typed protected regions can reserve author-declared source areas | Partial. Audit the existing caption-band contract and its mapping before declaring integration complete. |
| Existing local renderer consumes motion | Selected authoring compile and scene-build emit phased camera jobs; patterned-pixel test, six actual study renders | Verified for first camera path. |
| Boundary, settled-pose, hold-midpoint, adjacent and overlap evidence | Compiler sample schedule, overlap-interior test, indexed frame-counter extraction, PNG-to-picture checks | Verified for supported treatment; overlap is planned evidence when execution is unsupported. |
| Contact sheets, normal/quarter playback, exact lineage and tool versions | Engine evidence/receipt files, sampled browser review, source/job/output hashes and FFmpeg versions | Verified for retained study. Final producer commit/binary pin remains part of consumer adoption. |
| Reuse motion-check to distinguish holds from unexpected freezes | Existing `adapters::still_animatic::check_motion` has hold masks; Motioncraft currently uses independent hold/push pixel tests and indexed verification | Incomplete: no Motioncraft-to-existing-cadence-check bridge yet. Do not treat an element hold as a whole-frame allowance when other layers move. |
| No-clobber and partial-failure publication safety | Existing render/receipt conventions; negative scene-build tests; hydration tamper/traversal/new-only tests | Verified at tested REEL boundaries. |
| Matched original 15-second queue study, both profiles | 627-component study package; 1280x720 and 720x1280, 30 fps, 450 frames; same assets/audio/script/timing within profile | Verified. No deliberately degraded baseline. |
| Native, muted/captioned and reduced treatments; phone/desktop inspection | All six renders plus corrected v2 muted derivatives, native caption frames, sampled local browser playback at desktop/360-pixel phone width | Verified within documented sampled-review scope. |
| Core `.roles` findings and REEL dimension comparison | `reel-motioncraft-queue-comparison-2026-10-05.md`: 69→72 landscape, 70→73 portrait; Rhythm/Legibility gain, no Emotion/Execution regression | Simulated quality target met. No real viewer or creative approval claimed. |
| One verified package of intent, compiled inputs, assets, captions/audio, rendered media and evidence | `motioncraft-handoff-r1`; additive source-asset/render-audio kinds; REEL receipt/check verifies 627 components | Verified generic study transport; consumer production package remains unmet. |
| Clean hydration and reproduction through actual engine | Both clean profile workspaces reproduce six compiled JSONs, native stems, all 450 decoded picture frames and 44 review samples | Verified on same runtime; encoded-video cross-runtime identity not promised. |
| Exact synthetic CAIMITOS adapter baseline | Actual existing adapter script executed read-only; REEL render/check; independent PCM checker verifies sample 123–138 pulse | Verified baseline only. Existing adapter does not author/consume Motioncraft. |
| Actual CAIMITOS adapter/compiler changes in designated lane | Current entry diagnostic requires Asset Authority provisioning; no assigned writable consumer lane | Unmet/gated. Do not write shared admin or protected producer worktrees. |
| Future-episode authoring through consumer into selected render | REEL demo authoring works; no updated actual consumer example/command route yet | Unmet. A REEL-only fixture cannot substitute. |
| Explicit existing-revision upgrade and rollback with stable cue/shot/assets | REEL test upgrades direction and restores old authoring; no actual consumer versioned overlay/revision yet | Unmet at required consumer scope. |
| Illustrated hold/push motion canary with odd cue samples and exact E offset | Patterned REEL render tests and actual legacy CAIMITOS baseline are separate proofs | Unmet as one actual consumer motion pipeline. |
| Canonical consumer cache ingestion, authority registration and hydration | Generic study uses content-addressed local objects/hydration; no canonical CAIMITOS cache/authority handoff performed | Unmet. Worktree-only media are not a CAIMITOS handoff. |
| Consumer deployment/adoption instructions and compatibility results | REEL development commands/compatibility notes exist; CAIMITOS adoption not implemented | Unmet. Final handoff must pin real producer binaries/commits and tested invocation. |
| Delete duplicate fixture timing/review schedule only after equivalence | Scene-build test now reuses `read-then-push.json` instead of inline phase boundaries; native-duration template is derived. Before/after exact job SHA is `6f89aaf3e6ac9ac9a507381aa24ec375bb6a6b2696913a5900d9cb232094c896`; actual build test passes | Verified removal of duplicate test-fixture timing; compiled job includes unchanged review schedule. Independent pixel-index assertions remain intentional verification. |
| Required checks and final acceptance review | Focused core, scene, package and transport checks pass; most recent sampler change adds overlap coverage | Incomplete until cadence/caption and actual consumer changes are done and their required checks/final audit pass. |

## Six deliverables

1. Reuse map, contract documentation and authored example: available.
2. Validator/compiler and consuming preview: available, cadence-check bridge open.
3. Indexed evidence/contact sheets/lineage: available for supported camera path.
4. Matched both-profile/reduced packages: available and reproduced.
5. Regression/failure tests and final role review: checkpoint evidence available;
   final integrated review remains open.
6. Working CAIMITOS adoption, new/upgrade examples, motion canary, verified
   consumer package/cache lifecycle and compatibility: incomplete.

Deferred browser-rendering adapter, springs/overshoot and additional benchmark
families remain outside this goal. The temporary browser page is a local media
review tool, not a new production rendering adapter. No publication, push or
human approval is inferred from technical or simulated craft success.
