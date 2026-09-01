---
skill: simulate-contract
topic: reel-music-slice-b
date: 2026-09-01
gate_result: PASS
---

# Contract simulation: `reel-music` Slice B

## Inputs

- Specification: `docs/music-reconstruction-crate-proposal.md`, “Slice B —
  deterministic repair rendering.”
- Implementation: `crates/reel-music/src/edl.rs`,
  `crates/reel-music/src/evidence.rs`, `src/music_render.rs`, root CLI v0.2.27,
  tests, and release documentation.
- Execution: full Windows and WSL/Linux workspace suites; strict Clippy;
  role-schema validation; and the explicit real-FFmpeg integration test on
  Windows→WSL and native Linux.

## Gate token

```yaml
census-distribution: shared
gate-provenance: §S5.5-Sub-task-A
gate-status: PASS
attestation-by: Music Reconstruction Engineer
attestation-result: Slice B resolves cut-only plans at integer sample boundaries and proves exact unaffected mappings.
verification-by: Rights and Provenance Steward
verification-result: Execution is local, artifacts refuse overwrite, reports remain non-shareable, and technical success does not imply approval.
mechanism-distribution: shared
mechanism-type-shared: PASS
```

The census and mechanism distributions are both `shared`: provider-neutral EDL
and evidence semantics live in `reel-music`; external-process execution stays
in the root CLI FFmpeg adapter. Role names identify installed review lenses, not
human attestation or approval.

## Element diff

| # | Spec element | Actual implementation evidence | Severity | Result |
|---|---|---|---|---|
| 1 | Compile a validated edit plan into a resolved decision list. | `edl::write` revalidates repair/source lineage and emits schema `reel.music-repair-edl.v0.1`. | P2 | Match |
| 2 | Use deterministic edit identity. | Cuts, keeps, joins, duration, and output mappings use integer half-open per-channel sample ranges. | P2 | Match |
| 3 | Limit execution to specified behavior. | v0.1 executes ordered internal cuts only; non-cut mutating operations fail explicitly. | P2 | Match |
| 4 | Preserve declared change boundaries. | Each cut must equal one changed envelope; resolved keep segments cover every outside sample. | P2 | Match |
| 5 | Bind complete lineage. | EDL stores raw and canonical hashes for repair and source plus decoded PCM hash, format, and timebase. | P2 | Match |
| 6 | Keep FFmpeg outside the domain crate. | `src/music_render.rs` invokes the existing root `FfmpegAdapter`; `reel-music` gains no process or decoder dependency. | P2 | Match |
| 7 | Render without hidden time conversion. | FFmpeg graph uses `atrim=start_sample/end_sample`, timestamp reset, and concat; format/rate/channels and PCM codec are explicit. | P2 | Match |
| 8 | Refuse overwrite and partial publication. | EDL/evidence are atomically persisted; existing destinations fail; PCM renders to a same-directory temporary file and publishes by rename. | P2 | Match |
| 9 | Verify output duration. | Evidence requires exact candidate byte count from output samples × channels × sample width. | P2 | Match |
| 10 | Prove outside-region identity. | Every resolved source/output segment is byte-compared and independently hashed; any mismatch is a failing violation. | P2 | Match |
| 11 | Verify boundary continuity. | Each join records boundary delta, left/right RMS difference, waveform-window correlation, spectral distance, and DC-offset delta. | P2 | Match |
| 12 | Verify retained tails. | The mapped right segment must be exact and meet a minimum retained-sample length. | P2 | Match |
| 13 | Avoid overstating acoustic evidence. | Documentation calls correlation a waveform proxy, publishes thresholds, and disclaims perceptual or listening approval. | P2 | Match |
| 14 | Retain failed evidence for diagnosis. | A threshold failure writes candidate and evidence, then exits unsuccessfully. | P3 | Match |
| 15 | Produce private, path-free evidence. | Evidence artifact contains hashes/metrics without paths and declares `shareable: false`; CLI write report may contain a local path and is also non-shareable. | P2 | Match |
| 16 | Verify with a synthetic one-phrase repair. | Generated periodic u8 PCM removes four repeated periods; core tests prove exact output and deliberate mutation failure. | P2 | Match |
| 17 | Invoke real FFmpeg. | Explicit ignored integration test passes when run on Windows→WSL and native WSL/Linux and asserts exact candidate bytes plus evidence recheck. | P2 | Match |
| 18 | Preserve future scope. | Docs keep separation, models, transcription, notation, language adaptation, arrangement, comparison packaging, and listening selection deferred. | P3 | Match |

## Mismatches and residual risks

No blocking Slice B mismatch was found. Comparison-input packaging from the
proposal remains explicitly deferred; the private candidate and path-free
evidence required before it are implemented. The seam metrics are deterministic
engineering gates over a bounded window, not a psychoacoustic model. Real
private music still requires project-owned listening review and an actual human
decision record.

## Gate result

**PASS** — Slice B satisfies its deterministic cut-only rendering contract.
This gate does not approve a particular repair, grant rights, select a master,
or authorize private delivery or publication.
