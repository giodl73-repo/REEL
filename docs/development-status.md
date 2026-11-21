# Development status

REEL has an established media authoring/production codebase on `main` and active
work extending its workflow lifecycle. Use the [capability guide](capabilities.md)
for the main source's production areas and each linked contract for exact support.
Source availability, an installed runtime and adoption by a producing project
are separate states.

## Media format expansion plan

The [format-expansion plan](media-format-expansion.md) records the proposed next
direction: reusable semantic spoken-content modules assembled into technical,
executive, language and audience editions. It distinguishes existing foundations
from proposed applications, includes natural multi-speaker handoffs, and defines
a bounded first audio-only milestone.
This is a planning record, not an implemented programme runner or a recording.
The [phased PR sequence](media-expansion-pr-plan.md) starts with a source-backed
reuse/gap map, then separates spoken production, companion packages,
installation playback and interaction into bounded implementation PRs.

The first implementation slice adds [spoken-edition declaration validation](spoken-edition-v1.md):
modules, speaker cues, language scripts, selected performances and explicit
handoffs. It reads no referenced media or authority evidence and is not an
audio-only programme producer. Compilation, production, independent media/text
checking and retained edition integration remain later slices.

## Workflow orchestration

As observed on 2026-11-20, workflow development is active in a separate local
lane on `feat/workflow-handoff`.
It explores explicit task/result contracts, production handoffs, composition,
review and repair, retained history and native recovery across film/comic work.
The branch contains additions beyond the main checkout; this page does not
claim that those additions are merged, released or adopted by every producer.

Within that lane, `docs/workflow-orchestration.md` describes the architecture and
`docs/workflow-completion.md` records implementation and qualification scopes.
The inspected committed checkpoint was `cb67043`, with further uncommitted work.
No matching remote branch was advertised when checked, so these documents have
no verified public links here. Add public references after the owning lane
publishes them. This presentation cleanup leaves that work in its own lane and
does not copy draft contracts into main documentation.

The existing main-branch managed scene, presentation and episode jobs remain
documented under [use cases](use-cases.md) and [architecture](architecture.md).
Active orchestration work extends a framework that already produces and checks
media; its unfinished status is not the status of all REEL production.

## Other development references

- The [capability roadmap](tooling-roadmap.md) preserves a dated 2026-11-07
  source/adoption checkpoint. Its PR statuses describe that checkpoint and need
  fresh verification before being treated as current.
- Motioncraft's [authoring guide](motioncraft-authoring.md) and
  [execution checkpoint](motioncraft-execution.md) describe their documented
  scope; later phase work does not automatically expand that support.
- Remotion handoff packages and Blender/AI-video adapter declarations have
  distinct execution boundaries; see [architecture](architecture.md#engines-and-adapters).
- [CI](https://github.com/giodl73-repo/REEL/actions/workflows/ci.yml)
  qualifies the tested source and fixtures. Runtime installation, project
  cutover and creative acceptance require their own evidence.
