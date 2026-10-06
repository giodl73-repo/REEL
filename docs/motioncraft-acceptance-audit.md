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
| Reuse caption-band reservation | Shared existing caption geometry resolves an optional delivery-profile setting carried into selected jobs; real scene rendering fits/animates inside the region before output padding. Landscape/portrait and odd-height viewport tests pass; both retained study derivatives have 44 clear-band samples and exact unchanged native stems | Verified in REEL. Region coordinates are documented; legacy/post-compose cameras that could move the band reject. This reserves picture space; selected caption-text presentation and actual consumer adoption remain separate. |
| Existing local renderer consumes motion | Selected authoring compile and scene-build emit phased camera jobs; patterned-pixel test, six actual study renders | Verified for first camera path. |
| Boundary, settled-pose, hold-midpoint, adjacent and overlap evidence | Compiler sample schedule, overlap-interior test, indexed frame-counter extraction, PNG-to-picture checks | Verified for supported treatment; overlap is planned evidence when execution is unsupported. |
| Contact sheets, normal/quarter playback, exact lineage and tool versions | Engine evidence/receipt files, sampled browser review, source/job/output hashes and FFmpeg versions | Verified for retained study. Integrated r2 records actual REEL producer commit/binary/tool hashes and retains the binary snapshot; actual consumer adoption still remains. |
| Reuse motion-check to distinguish holds from unexpected freezes | `reel-scene-cadence` reuses the existing luma analyzer/thresholds on exact native frame spans after receipt verification; all six retained study reports in `cadence-r1` pass; patterned render and frozen-motion tests pass | Verified for unlayered still camera treatments. Layered compositions explicitly require separate analysis and receive no whole-frame hold allowance. |
| No-clobber and partial-failure publication safety | Existing render/receipt conventions; negative scene-build tests; hydration tamper/traversal/new-only tests | Verified at tested REEL boundaries. |
| Matched original 15-second queue study, both profiles | 627-component study package; 1280x720 and 720x1280, 30 fps, 450 frames; same assets/audio/script/timing within profile | Verified. No deliberately degraded baseline. |
| Native, muted/captioned and reduced treatments; phone/desktop inspection | All six renders plus corrected v2 muted derivatives, native caption frames, sampled local browser playback at desktop/360-pixel phone width | Verified within documented sampled-review scope. |
| Core `.roles` findings and REEL dimension comparison | `reel-motioncraft-queue-comparison-2026-10-05.md`: 69→72 landscape, 70→73 portrait; Rhythm/Legibility gain, no Emotion/Execution regression | Simulated quality target met. No real viewer or creative approval claimed. |
| One verified package of intent, compiled inputs, assets, captions/audio, rendered media and evidence | Integrated `motioncraft-handoff-r2` inventories 840 components: eight newly produced renders, cadence/caption evidence, four hydration profiles and producer/runtime/assembler pins. REEL receipt/check passes; original r1 remains intact | Verified REEL transport; actual consumer production package/cache adoption remains unmet. |
| Clean hydration and reproduction through actual engine | Four clean r2 workspaces independently compile/build/check and reproduce six exact compiled JSONs, four exact WAVs, all 450 decoded frames and 44 review PNGs each. Prior receipt replay regression passes | Verified on pinned same runtime; actual CAIMITOS clean consumer route still required. Encoded-video cross-runtime identity not promised. |
| Exact synthetic CAIMITOS adapter baseline | Actual existing adapter script executed read-only; REEL render/check; independent PCM checker verifies sample 123–138 pulse | Verified baseline only. Existing adapter does not author/consume Motioncraft. |
| Actual CAIMITOS adapter/compiler changes in designated lane | Current entry diagnostic requires Asset Authority provisioning; no assigned writable consumer lane | Unmet/gated. Do not write shared admin or protected producer worktrees. |
| Future-episode authoring through consumer into selected render | REEL demo authoring works; no updated actual consumer example/command route yet | Unmet. A REEL-only fixture cannot substitute. |
| Explicit existing-revision upgrade and rollback with stable cue/shot/assets | REEL test upgrades direction and restores old authoring; no actual consumer versioned overlay/revision yet | Unmet at required consumer scope. |
| Illustrated hold/push motion canary with odd cue samples and exact E offset | Patterned REEL render tests and actual legacy CAIMITOS baseline are separate proofs | Unmet as one actual consumer motion pipeline. |
| Canonical consumer cache ingestion, authority registration and hydration | Generic study uses content-addressed local objects/hydration; no canonical CAIMITOS cache/authority handoff performed | Unmet. Worktree-only media are not a CAIMITOS handoff. |
| Consumer deployment/adoption instructions and compatibility results | REEL development commands/compatibility notes exist; CAIMITOS adoption not implemented | Unmet. Final handoff must pin real producer binaries/commits and tested invocation. |
| Delete duplicate fixture timing/review schedule only after equivalence | Scene-build test now reuses `read-then-push.json` instead of inline phase boundaries; native-duration template is derived. Before/after exact job SHA is `6f89aaf3e6ac9ac9a507381aa24ec375bb6a6b2696913a5900d9cb232094c896`; actual build test passes | Verified removal of duplicate test-fixture timing; compiled job includes unchanged review schedule. Independent pixel-index assertions remain intentional verification. |
| Required checks and final acceptance review | Focused core, scene, package and transport checks pass; most recent sampler change adds overlap coverage | REEL r2 package/check and four clean reproductions pass; workspace library check passes 169 tests (three default ignored library tests remain explicit), scene delivery passes all 20 including FFmpeg, and transport passes six. Actual consumer changes, compatibility and final integrated audit remain unmet. |

## Six deliverables

1. Reuse map, contract documentation and authored example: available.
2. Validator/compiler and consuming preview: available, native-frame cadence bridge verified for unlayered camera shots.
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

## Current blocking audit

The required CAIMITOS lane gate has persisted across successive cadence,
caption and package goal turns. A fresh read-only entry diagnostic again reports
`animation-vfx` as `requires-dedicated-worktree`, with provisioning/clearance
required from Asset Authority. Registry branch/worktree are null. Shared admin
and other live lanes are protected by the repository's administrative mutex;
none is an assigned writable Motioncraft consumer lane.

All tracked render/check processes for r2 and its four clean reproductions
completed successfully. The REEL package checkpoint role review records finding
dispositions in `reel-motioncraft-package-checkpoint-2026-10-05.md` under
`signals/roles/check`. Next required work changes the actual CAIMITOS
adapter/compiler, versioned episode route and canonical cache/authority state.
Those actions cannot proceed under the current gate, and a REEL-only overlay
or unregistered checkout would not satisfy the plan. A designated registered
integration lane with the necessary owned paths is the required external change.
Final consumer acceptance and the full goal remain unproven.

### Additional runtime verification

The legacy mixed-media smoke initially failed twice in WSL FFmpeg with
`No space left on device`, despite available storage. A clean baseline 4effab1
passed. A shared fixture comparison subsequently proved identical FFmpeg
arguments (excluding output paths), all 144 decoded picture frames and decoded
audio bytes between baseline and Motioncraft. Two unmodified Motioncraft smoke
runs then passed. No production flags or backend configuration were changed;
temporary diagnostic instrumentation was removed. The earlier failures remain
intermittent and unexplained; no deterministic command/output regression was
reproduced. This single fixture does not establish general legacy compatibility.
Evidence is retained in `target/motioncraft-mixed-diagnostic-r1`.

Scene rendering uses native Windows FFmpeg; cadence invokes WSL FFmpeg 8.0.1.
Refreshed cadence reports record actual backend/version, separately from the
native media producer. The subsequent package checkpoint records both identities.
Actual CAIMITOS adoption, new/existing episode upgrades and canonical cache
handoff remain required before this goal can be completed.

### Verified r3 analyzer provenance checkpoint

`target/motioncraft-handoff-r3` passes the actual REEL package receipt/check:
840 components; package SHA256
`20760e61f00e9ebaa10b99d3d1e714bc3ba742c8cdaf1715f33184b75a514ccb`.
All eight refreshed cadence reports pass and match the separate actual analyzer
backend/version pin. 830 unchanged component hashes match r2; only the selected
cadence reports and producer/hydration metadata change. Four clean r3 input
hydrations verify every copied input and match the previously reproduced r2
inputs; no compiled/rendered outputs were copied. This refresh does not claim
four new rendered reproductions: r2 retains that rendering proof.

Actual CAIMITOS integration remains blocked: its animation-vfx entry check still
requires Asset Authority to provision or clear the dedicated worktree. The
bertica-session-handoff skill explicitly keeps such lanes gated until
administration establishes them. No CAIMITOS source, registry or canonical cache
was changed. Next required work is actual adapter/compiler adoption, future
episode and existing-version upgrade/rollback examples, the illustrated odd
sample canary, canonical authority/cache handoff and final integrated review.
The goal must remain incomplete until those requirements pass.
