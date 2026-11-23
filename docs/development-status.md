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
checking and retained edition integration have separate qualification scopes.
The [control compiler](spoken-edition-compilation-v1.md) additionally resolves
selected alignment metadata into sample placements, chapters and consumed-recipe
identities. It opens no source audio and records media/authority as unverified.
Audio assembly, independent output/text checks and retained asset integration
remain later slices.

## Workflow orchestration

The [workflow source entry](workflow-entry.md) links the landed manual, film
inspection, protected native lifecycle, owned film CLI and bounded API/proxy
paths. The [film-first PR plan](workflow-pr-plan.md) defines their qualification
and the remaining work. These public source entries supersede the earlier
local-branch-only navigation checkpoint.

Source landings include [manual exchange #386](https://github.com/giodl73-repo/REEL/pull/386),
[film metadata #388](https://github.com/giodl73-repo/REEL/pull/388),
[native handoffs #392](https://github.com/giodl73-repo/REEL/pull/392),
[retained film tasks #397](https://github.com/giodl73-repo/REEL/pull/397),
[owned film CLI #399](https://github.com/giodl73-repo/REEL/pull/399) and
[API/proxy execution #402](https://github.com/giodl73-repo/REEL/pull/402).
Complete film review/issue/fix/rebuild, dedicated service/speech integration, automatic
native provider orchestration, packaged project adoption/rollback and episode-scale
qualification retain their separate gates. A source merge does not install or
adopt a production profile, approve a candidate or qualify all of Season 5.

The first W4 [cel-ID review-copy producer](film-cel-review-v1.md) annotates an
existing scene-delivery review on actual delivered frame ranges, preserves its
audio/timestamps and emits an exact target map. It leaves issue/fix integration,
affected rebuilds and native workflow dispatch open.

The [visual production workspace requirements](workflow-ux-requirements.md)
audit existing REEL editors and current git-asset catalogue extension gaps.
They propose a live cel/cue/page board and a generic tool/preview handoff, keeping
production semantics in REEL. The first [read-only board](../crates/reel-workflow-ui/README.md)
implements pinned local cels/versions/cues and explicit journal observations.
Browser visual acceptance, project projection and native preview integration
remain open; this is not an adopted Season 5 production dashboard.

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
