# Motioncraft Phase 2 implementation record

Goal: complete the five work packages in CAIMITOS's
`projects/animation-vfx/MOTIONCRAFT-PHASE-2.md`, including actual consumer and
complete bilingual episode qualification. This record is a checkpoint, not a
completion claim.

## P2.1 — native Windows cadence

Implemented a dedicated native analyzer resolver. `REEL_CADENCE_FFMPEG` is an
explicit file override; otherwise PATH resolves the native executable once.
All measurements use that canonical binary; version, path and SHA-256 identify
it. A post-analysis hash check rejects replacement during execution. No WSL or
shell transport is used. Existing luma thresholds, native frame trimming and
hold semantics remain unchanged; older non-Motioncraft adapters are unaffected.

Regression coverage includes missing/directory executable rejection, a real
FFmpeg frozen clip that fails moving expectations and passes held expectations,
and the existing rendered phased-camera/reduced-motion and caption-profile
tests. CI now explicitly runs these real-media tests on Windows and Linux.

Role findings applied by the current assistant, not independent reviewers:
animation direction requires real decoded moving/hold evidence; provenance
requires exact executable identity and no invisible fallback. Both are covered
in the implementation. Local analyzer evidence contains machine paths and must
remain separate from sanitized publication receipts.

## P2.2 — matched comparison engine

Added `reel-motioncraft-compare` and the public `motioncraft_comparison` module.
One exact directed scene job produces all three treatments through the native
engine. The package retains dependencies, original source bytes, verified native
stems, variant receipts, cadence reports, indexed frames and synchronized review
playback. The checker independently derives variant expectations, reproduces
frame metrics and checks saved pixels against exact decoded indices. Legacy
camera migration is explicit; no artwork/audio selections change.

See [the comparison contract](motioncraft-comparison.md). Native clock validity,
cadence results and human creative judgment remain separate. The tests exercise
a real illustrated moving camera, exact still/reduced pixels, source withdrawal,
whole-package relocation, no-clobber and tampered audio rejection. CI runs the
portable comparison on Windows and Linux.

## P2.3 — focal anchor and bounded pan

Optional normalized `focal_anchor` and paired phase `pan_from`/`pan_to` compile
through the shared Direction API into actual source-sense perspective geometry.
Pan and zoom share bounded phase easing. Fixed-anchor affine geometry permits
endpoint validation of protected regions and source-edge coverage over every
phase. Holds, one-frame moves, nonfinite values, partial point pairs and endpoint
discontinuity reject. Native fitting changes phase frames only. Cadence considers
pan as well as zoom; reduced motion restores identity composition. Directions
without the new fields retain exact Phase 1 expressions and serialization.

Unit coverage includes protected-region/source-edge rejection, continuity,
reduced identity, short fitted phase rejection and different native sample/FPS
spans. Real-media tests cover pure pan, off-center versus centered framing,
identity reduced frames, exact stems, and caption reservation at landscape,
portrait and odd-sized picture-region geometries. See the
[execution contract](motioncraft-focal-camera-contract.md).

Role findings applied by the current assistant: protect family/reading regions
throughout the path; preserve audio and approved image pixels; expose intended
geometry rather than inferred face targeting; retain legacy rollback. Actual
principal approvals and creative choice remain separate.

## P2.4 and actual consumer checkpoint — 2026-10-06

Layer validation now proves source contribution through every later composite stage, checks per-frame visibility and active-span timing, and prevents hidden motion from borrowing a pass from visible static content. Real-media tests cover freeze/hold classification, partial alpha, complete occlusion, transparent regions, contradictory composition and outside-span contribution. Inconclusive intervals retain a null result.

The actual CAIMITOS consumer exposed a lost final overlay frame and a JSON feature-combination parsing failure. REEL b674148 corrected native frame time bases and uses the native JSON/YAML job reader consistently. Windows, Linux and FFmpeg CI all passed at that engine revision (run 37425306578).

CAIMITOS pins b674148. Its freshly hydrated 84-file layer handoff reproduces the producer report exactly; its freshly hydrated 272-file comparison passes the actual native consumer checker with all three cadence results and exact stems. Retained consumer reports identify historical producers without retagging them. Current adapter test targets, format and all-target Clippy pass.

Episode 1 intake: all 25 scenes in both languages compiled from isolated hash-bound metadata and passed native cache preflight. This is not episode qualification: none of these intake jobs has captions, and soundtrack/presentation parity still requires reconciliation against the selected full packages. One historical Scene 004 English matrix root pin is stale; the current revision is recorded explicitly. The selected Spanish Scene 004 clock differs by 215040 samples, matching the existing native-clock correction rather than an invented retime.

Later CAIMITOS checkpoints retain actual focal authoring/upgrade/rollback proof and fresh cache consumer checks. Five formal technical authority receipts now cover 1667 retained assets. Native word evidence covers all 282 selected body cues and exact PCM joins for all 50 scene/language positions. Caption segmentation remains unreviewed: 268 cues admit 810 candidates under the provisional phone policy; a second constrained alignment adds a candidate for Spanish F1-089 while retaining other failures. These results do not qualify a complete episode.

Remaining: exact full-episode freeze, caption and presentation bindings, supported profile/layer dispositions, all 12 delivery combinations and contextual review, reviewed PR integration and main shipping. Do not declare Phase 2 complete from engine tests or intake planning.

## P2.5 explicit picture viewport checkpoint — 2026-10-06

Selected poem presentations need reserved side-panel space; portrait needs a separately composed picture/text area. An optional output-pixel `picture_region` now propagates from the delivery profile to the native job. The shared geometry resolver checks output bounds, overflow, nonempty regions and reserved caption-band containment. Absent region preserves previous serialization and behavior. Camera/protection coordinates remain local to the picture viewport; selected source hashes remain unchanged.

The renderer fits and moves picture pixels inside the viewport before final padding and overlays. Review evidence records the resolved viewport, and cadence measures and identifies its explicit region. Real native Windows FFmpeg tests prove landscape/portrait containment, holds, camera travel and exact four-stem identity. The regular all-target/all-feature suite and warnings-denied Clippy pass locally; CI includes the native viewport test on both operating systems.

CAIMITOS pin propagation, actual selected poem/profile text binding and final episode review remain separate open gates. The viewport itself neither creates nor qualifies caption/poem text, nor automatically migrates old camera treatments or profile-specific effect carriers.
