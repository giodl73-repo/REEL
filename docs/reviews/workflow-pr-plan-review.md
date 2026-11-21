# Role review: film-first workflow PR plan

Review method: author-applied inspection of the proposed PR sequence using
[REEL's role rules](../../.roles/ROLE.md). This is a plan review, not an
independent review, creative approval or production qualification.

The review covers story, animation, editing, sound, platform/audience,
rights/provenance, lyric/vocal alignment and preservation of musical form/uses.
It proposes no source decomposition, musical reconstruction, re-orchestration
or translation. Those operations need their own relevant roles and authorities.

| Finding | Role / severity | Problem in the proposed sequence | Incorporated change |
| --- | --- | --- | --- |
| R1 | [Platform and Audience](../../.roles/platform-audience.md), blocking | The first film release promised a project handoff while episode-scale requirements sat in a later PR. | W5 must demonstrate its advertised project scale and delivery editions. Required W6 work moves before that claim; existing accepted producers can remain the production path. |
| R2 | [Story Director](../../.roles/story-director.md), high | Reuse of existing work and preservation of canon/approval subjects were underspecified. | W2 maps exact versions and stable identities without regeneration or active-profile changes. Story order, narrator/poet roles and continuity remain explicit. |
| R3 | [Animation Director](../../.roles/animation-director.md), high | Style, cel density and HTML/CSS/SVG were missing from acceptance. | W2 records these settings and producer capability requirements. W3 must implement or reject them explicitly; future animation fields do not imply executable support. |
| R4 | [Editor](../../.roles/editor.md), high | A small render/recovery test could omit real opening, ending, breaths and complete-episode rhythm. | Project acceptance preserves complete selected components and ordered boundaries. Local fixes require affected-context review and exact consumer/language invalidation. |
| R5 | [Sound Designer](../../.roles/sound-designer.md), high | Existing takes, language-local durations and caption/conform correspondence were not explicit. | Reuse selected recordings; retain each language's measured clocks and semantic triggers. Captions use conform/take receipts; listening is a separately recorded modality. |
| R6 | [Lyrics and Vocal Adaptation Editor](../../.roles/lyrics-vocal-adaptation-editor.md), high | Timing adaptation could be mistaken for text or performance authority. | Preserve canonical, performed and translated text layers and their authorities. Do not infer performed words from a generation request or alter text to fit a clock. |
| R7 | [Score and Arrangement Director](../../.roles/score-arrangement-director.md), high | Complete musical form and actual-use credits could be lost in a generic component test. | Preserve selected complete musical material and measured alignment; credits join actual selected performances/score uses. No musical revision or approval is inferred. |
| R8 | [Rights and Provenance Steward](../../.roles/rights-provenance-steward.md), blocking | Service mocks, asset registration and software merge could be read as account qualification, creative approval or production adoption. | W3b distinguishes transport from service qualification. W5 requires owned adoption/rollback and privacy-safe evidence. Public source PRs contain synthetic fixtures; project evidence remains private. |

## Implementation feasibility findings

These are author observations from the dependency inspection, not additional
reviewer identities:

- The integration branch spans core contracts, provider transport, native
  lifecycle, producer changes, comics, SDK copies and evidence documentation.
  Submit bounded extractions from current main, not that branch wholesale.
- The native adapter currently enables comic dependencies unconditionally.
  Film-only build/test acceptance requires removing or gating that coupling.
- The native SDK pin differs from the existing producer SDK. Keep source-pin
  intake visible and avoid implicitly changing accepted producers' dependency
  graph. Split SDK intake from adapter logic where needed for review.
- Unfinished native composition has real registration/close bounds. Preflight
  the complete declared lifecycle before effects; do not raise limits or call a
  rendered-but-unretained result complete.

## Preserved tension and disposition

Early delivery favors orchestration of accepted producers; complete native
qualification favors an end-to-end replacement path. Both concerns remain in
the plan. The disposition is explicit execution modes and evidence per mode,
with project-scale requirements gating any production-readiness claim.

Comic completeness remains important for a later publication release. It does
not gate film delivery and must not consume the film acceptance sequence.

The revised [PR plan](../workflow-pr-plan.md) incorporates these findings.
Implementation, project acceptance, actual craft review and release decisions
remain open. This document supplies no human approval and no blanket assertion
that the implementation branch is ready to merge.
