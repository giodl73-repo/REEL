# Pulse 06 — Music reconstruction foundation

Implemented REEL v0.2.26 after the BERTICA soundtrack-repair pilot showed that
minimal lyric repair, same-composition language performance, and later
re-orchestration need one recoverable musical lineage rather than unrelated
generation prompts.

## Delivered

- provider-neutral `reel-music` workspace crate with no DSP/model runtime;
- strict `reel.music-source.v0.1`, `reel.music-neutral-plan.v0.1`, and
  `reel.music-repair.v0.1` contracts;
- exact raw-file and decoded-PCM identity, byte-count verification, integer
  sample/PPQ timebases, explicit rounding, and canonical contract hashing;
- atomic neutral keep/lock planning and exact candidate re-verification;
- typed repair grammar with complete changed/locked coverage, asset evidence,
  overlap and lock-trespass rejection, and mandatory specialist role routing;
- four new music-governance roles with explicit advisory boundaries;
- synthetic raw-PCM and repair fixtures containing no consumer-private content;
- repaired cross-platform exact-lyric fixture policy; and
- passing Windows and WSL/Linux workspace, CLI, contract, Clippy, formatting,
  role-schema, and whitespace gates.

## Boundary

The foundation validates evidence and plans. It does not decode containers,
render edits, measure acoustic seams, analyze stems, correct lyrics, export a
score, translate, re-orchestrate, invoke a model, infer approval, or authorize
release. Every local v0.2.26 report declares `shareable: false`.

## Next pulse

Compile the synthetic repair into a canonical edit decision list, render it
through the existing FFmpeg boundary, and verify exact outside-region identity
plus discontinuity, phase, ambience, loudness, spectral, and tail evidence.
