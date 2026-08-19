---
skill: roles-check
topic: reel-choreography-render-scale-v030
date: 2026-08-17
roles_used: 4
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# Roles check: REEL choreography render scale v0.2.30

## Artifact identification

- Type: renderer and choreography-compiler capability
- Scope: performer visibility, bounded pose-to-pose path sampling, and long WSL command transport
- Evidence: library/CLI tests plus a verified seven-performer, one-prop, 15-second customer proxy

## Roles selected

1. Animation Director — motion language, sprite continuity, and feasible execution.
2. Editor — authored sampling rhythm and review-proxy efficiency.
3. Platform and Audience — review profile, silent comprehension, and export disclosure.
4. Story Director — whether mechanics preserve causality rather than rewrite the scenario.

## Review findings

### Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Inclusive visibility windows solve entrances and exits without deleting complete character paths. | P3 | choreography contract | Keep windows beat-bound and validated against every action. |
| 2 | Performer and prop path subdivisions make pose-to-pose intent explicit while retaining the original marks and handoffs. | P3 | asset binding | Use low subdivision counts deliberately for graphic animation and proxies, not as an undocumented quality downgrade. |
| 3 | Seven-performer smooth rendering remains too slow for an interactive review loop even after command transport succeeds. | P2 | rendering performance | Profile sprite overlay reuse and avoid reopening identical assets per segment before promoting smooth full-resolution output as routine. |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Binding-level sampling lets an editor choose intentional pose beats without changing source choreography timing. | P3 | compiler | Preserve the default subdivision counts for backward compatibility. |
| 2 | The Karts proxy fell from an over-sampled 37 KB manifest to a 15 KB pose-to-pose manifest and rendered in a practical review window. | P3 | customer evidence | Add keyframe/input counts to future compiler reports for easier cost forecasting. |
| 3 | Sampling bounds prevent an accidental render graph explosion while still permitting denser action where justified. | P3 | validation | Keep invalid zero and excessive values as hard errors. |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The customer proxy declares 640×360, 12 fps, silent, legacy motion, and private-review scope instead of posing as a delivery master. | P3 | receipt | Keep profile facts in path-free receipts. |
| 2 | WSL script transport removes a Windows-only command-line failure without exposing shell scripts as durable artifacts. | P3 | FFmpeg adapter | Continue shell-quoting every argument before writing the temporary script. |
| 3 | Silent output is understandable at the causality level because the action and puck remain visible, but accessibility/publication work is not implied. | P3 | customer proxy | Treat captions and sound as later editorial choices, not renderer defaults. |

### Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Visibility preserves the changing cast rather than grouping principals who were not simultaneously in the tracked action. | P3 | performer windows | Keep entrance and exit semantics available to all production bindings. |
| 2 | Sampling changes execution density only; beat identity, possession order, and production binding remain fixed. | P3 | compiler contract | Preserve this separation in future optimization work. |
| 3 | The one-prop handoff chain still carries the story causality through reduced sampling. | P3 | generated manifest | Continue validating handoff ownership independently of render density. |

## Synthesis

Roles reviewed: 4  
P1 blockers: 0 | P2 issues: 1 | P3 notes: 11

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: V0.2.30 now makes complex sprite blocking launch reliably and render useful pose-to-pose proxies, but smooth multi-performer performance remains a targeted optimization task.

Cross-role consensus: Sampling density belongs to execution policy, while choreography timing, visibility, identity, and handoffs must remain authoritative.

## Amendments

1. Profile and reduce repeated identical sprite inputs in the smooth FFmpeg backend.
2. Add compiler report counts for emitted performer and prop keyframes so render cost is visible before execution.
3. Keep customer receipts path-free and label proxy resolution, frame rate, sound, and motion quality explicitly.
