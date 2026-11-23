# Spoken-content expansion plan: role-checklist review

**Subject:** [Media format expansion plan](../media-format-expansion.md) at
`ff71797b14990b799eb8655ac226d599a2618519`, with the corrections below.
**Method:** one assistant applying six relevant [REEL role checklists](../../.roles/ROLE.md).
This is a review of planning clarity and acceptance criteria. No independent
reviewers, produced-audio review or creative/rights approval is claimed.

## Scope and lenses

Reviewed the plan's reusable semantic modules, executive/technical English and
Spanish editions, speaker handoffs and first bounded milestone. Consulted
[principles](../../.pitfall/reel-principles.md),
[invariants](../../.pitfall/reel-invariants.md),
[pitfalls](../../.pitfall/reel-pitfalls.md),
[voice-performance direction](../voice-performance-v0.2.15.md),
[cue-relative assembly](../cue-relative-assembly-v0.1.md) and
[speaker-aware presentation](../speaker-caption-presentation-v0.2.9.md).

Animation direction, music reconstruction and score/arrangement were not applied
as full checklists: this milestone introduces no picture production, source
decomposition or musical arrangement. Those lenses become necessary when such
work enters a later slice. Lyrics/vocal adaptation questions are applied to spoken
word fidelity, language authority and prosody, without inventing note underlay
requirements for speech.

## Findings and disposition

**Medium** means a missing planning requirement could permit a misleading or
unreviewable edition. **Low** concerns scope clarity. These severities do not
claim defects in an implemented producer.

| ID | Severity / lens | Finding at baseline | Correction |
| --- | --- | --- | --- |
| S01 | Medium — Story Director; Editor | Coherent joins were required, but omitting technical detail had no explicit dependency/qualification check. A smooth executive cut could change the meaning of a claim. | **Closed in plan:** declare context dependencies, preserve qualifications/uncertainty, and author a bridge or reject unresolved references. Do not imply an exchange that never occurred. |
| S02 | Medium — Lyrics and Vocal Adaptation; Sound Designer | Word-fidelity review was named without explicitly separating requested text, observed performance and transcription/alignment evidence. | **Closed in plan:** keep those layers and approved corrections distinct; mark uncertainty and listen for exact wording, pronunciation and stress per language. |
| S03 | Medium — Platform and Audience; Sound Designer; Editor | Four editions were named without requiring a concrete listening/output brief or defining what accessible audio-only text access preserves. | **Closed in plan:** author duration, device/context, format, sample/channel, mix/loudness and transcript/chapter requirements before production; review whole programmes on the declared setup. |
| S04 | Medium — Rights and Provenance | Voice scope and rights review were mentioned, but reuse in a new audience/assembly, implied endorsement and provider-transfer evidence were underspecified. | **Closed in plan:** require operation-specific authorities and reuse scope, distinguish recording permission from new-assembly use, separate private evidence from portable receipts and retain distinct decision gates. |
| S05 | Medium — Editor; Sound Designer; Rights and Provenance | Positive acceptance cases did not require rejection of invalid context, stale timing, clipped protected material, undeclared overlap or unscoped reuse. Review identity was also implicit. | **Closed in plan:** list these negative cases, require no final package for invalid input, and bind review to exact edition/scripts/takes/handoffs/mix so affected changes stale the review. |
| S06 | Low — Editor | The concept supports several speakers inside a module, but the proposed three-module fixture only demonstrated joins between modules. | **Closed in plan:** explicitly limit the first fixture to distinct host/expert roles and between-module handoffs; leave within-module multi-speaker coverage for a later case. |

## Checklist results and remaining evidence

| Lens | Planning result after correction | Evidence still needed in production |
| --- | --- | --- |
| [Story Director](../../.roles/story-director.md) | Each edition has a purpose, opening/landing and dependency checks; summary selection must preserve upstream meaning. | Actual scripts, source authority and narrative review of every assembled edition. |
| [Editor](../../.roles/editor.md) | Order, omitted-detail joins, pause policy and review invalidation are explicit; reuse remains conditional on context. | Listen to complete editions and joins; measure pacing against the authored brief. |
| [Sound Designer](../../.roles/sound-designer.md) | Recorded silence and added gaps are distinct; breaths/tails, intentional overlaps, mix policy and text access are covered. | Real take measurements, intelligibility/mix checks and listening acceptance. Sample-clock adjacency cannot prove naturalness. |
| [Platform and Audience](../../.roles/platform-audience.md) | Executive/technical purpose and language are explicit; the first fixture must choose concrete device/output/accessibility requirements. | Filled delivery briefs, accessible transcripts and target-device review; no delivery service is selected here. |
| [Lyrics and Vocal Adaptation Editor](../../.roles/lyrics-vocal-adaptation-editor.md) | Language-local scripts, actual words, evidence and decisions remain separate; timing does not establish translation or pronunciation approval. | Authorized adaptations, observed performance/uncertainty records and language-specific listening decisions. |
| [Rights and Provenance Steward](../../.roles/rights-provenance-steward.md) | New combinations need scoped text/voice authority and privacy-safe evidence; technical success grants no consent or release. | Actual authority/consent and provider evidence for the selected operation, plus separate selection/delivery/release decisions. |

The reuse/pacing tension remains deliberate: exact takes reduce repeated
production, but a natural handoff or truthful summary may require a newly authored
bridge or performance. The plan prioritizes meaning and listening quality over
maximizing the number of reused takes. The executive edition is not assumed to
inherit the technical edition's review.

## Validation and disposition

Checked local Markdown destinations for the changed plan and this record, and
ran `git diff --check`. The existing Mermaid diagram is unchanged. Hosted
documentation checks apply before landing.

All six identified planning findings are corrected. Implementation, fixture
production and the evidence in the table remain future work; none is represented
as passed by this review. No speech generation, audio audition, private-media
inspection, provider execution, cache operation or release was performed.
