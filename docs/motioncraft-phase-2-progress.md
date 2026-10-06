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

Pending: Linux CI evidence, CAIMITOS pin/adoption verification, P2.2 comparison
orchestration, P2.3 focal camera, P2.4 layer validation and P2.5 complete episode
qualification. Do not declare Phase 2 complete from P2.1 tests.
