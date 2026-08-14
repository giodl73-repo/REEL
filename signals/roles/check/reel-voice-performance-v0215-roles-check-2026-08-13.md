---
skill: roles-check
topic: reel-voice-performance-v0215
date: 2026-08-13
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.15 voice-performance role review

Artifact type: provider-neutral voice-performance schema, compiler, CLI,
receipts, tests, fixture, and production documentation.

Selected roles: Sound Designer for executable delivery and listening gates;
Story Director for source fidelity and dramatic beats; Editor for phrase
boundaries and timing; Platform and Audience for portable review behavior; and
Animation Director for timing/renderer interoperability.

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Action labels were not engine-native controls and could be mistaken for executed emotion. | P2, resolved | compiler | List `action` as advisory-only for both current engines. |
| 2 | Chatterbox intensity can exceed its supported exaggeration ceiling. | P2, resolved | compiler | Clamp deterministically and disclose the exact clamp in each span. |
| 3 | A performance plan does not itself prove that an audio chunk was rendered or emotionally successful. | P3 | receipt | Keep `human_listening_required`; add a separately bound render receipt in a later pulse. |

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Performance direction must not rewrite canonical narration. | P2, resolved | sidecar validation | Bind Unicode offsets and substring hashes to exact inline cue text. |
| 2 | The fixture originally began at the peak and did not prove contrast before escalation. | P2, resolved | fixture | Add a neutral setup before the explosive interruption and warning. |
| 3 | Cultural register can guide a human reading but cannot certify Cuban authenticity. | P3 | directing context | Keep register advisory and require principal listening review. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Gaps or overlaps could drop or duplicate words during chunking. | P2, resolved | validation | Require complete, ordered, nonoverlapping span coverage. |
| 2 | Double-declared boundary pauses could drift if their values disagree. | P2, resolved | validation | Reject contradictory adjacent pause declarations. |
| 3 | Uniformly high intensity and scene-level contrast are not yet scored automatically. | P3 | continuity QC | Add the requested continuity/QC pulse after real auditions establish useful thresholds. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Engine-specific parameters must remain portable and inspectable. | P2, resolved | plan | Separate requested direction, native parameters, deterministic operations, and advisory-only fields. |
| 2 | Rechecks formerly trusted hashes without revalidating the sidecar against the current manifest. | P2, resolved | plan check | Re-run manifest and exact-span validation during receipt verification. |
| 3 | This pulse does not compose comparison slates or captions for silent review. | P3 | audition workflow | Reuse the existing comparison contracts in the later private-audition pulse. |

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Performance replacements can alter shot and caption timing. | P3 | integration | Bind rendered durations and rerun downstream duration-drift checks after synthesis. |
| 2 | REEL should not acquire a runtime dependency on a consumer project or private voice asset. | P2, resolved | fixture/architecture | Keep the fixture sanitized and the compiler provider-neutral. |
| 3 | Emotional direction must not silently change camera or motion continuity. | P3 | integration | Treat this sidecar as sound timing input; make visual changes through normal manifest review. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 9 resolved | P3 notes: 6

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: a compiled plan is auditable direction, not evidence of a rendered
or convincing performance. Cross-role consensus: exact text and timing must
remain bound through synthesis, and Bertica/Herman listening remains the gate.

## Amendments

1. Applied: disclose controlled action, energy, onset, stress, pitch, breathiness,
   and cultural register as advisory wherever the engine cannot execute them.
2. Applied: add neutral-to-peak contrast to the sanitized fixture and make
   re-verification re-run exact-span validation.
3. Follow-up: add a render-result receipt with output hash/duration and continuity
   checks when the local audition composer is connected; do not claim that the
   current plan command rendered audio.
