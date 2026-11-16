# Capability and adoption roadmap

Checkpoint: 2026-11-07. This is a source/adoption map, not a release promise or a
report on any private project's readiness. See [use cases](use-cases.md) for
workflow choices and [architecture](architecture.md) for ownership boundaries.

## Available source

| Area | Entry point | What still needs an explicit decision or qualification |
| --- | --- | --- |
| Planning, conform and media checks | [Quickstart](getting-started.md), [scene delivery](scene-delivery-v0.1.md), [media comparison](native-media-parity.md) | Actual source coverage, runtime capabilities, output profile and visual/listening acceptance |
| Public Tig adapters | [Integration guide](tig-public-integration.md), [vendor pin](../vendor/README.md) | Project definitions/admission, existing stores and actual runtime adoption; source merges do not install tools |
| Native scenes and language variants | [Scene authoring](scene-authoring-v1.md), [Tig compiler](tig-native-scene.md) | Complete supported inputs and explicit bindings; wider semantic parity is not inferred from file availability |
| Managed scene/presentation/episode jobs | [Job runner](tig-job-runner.md), [presentation production](tig-presentation-production.md), [episode jobs](tig-episode-jobs.md) | Enrolled budgets/stores, Windows protection, exact source/runtime pins and complete applicable ownership coverage |
| Native browser rendering | [Browser worker](tig-browser-native-runtime.md) | Admitted browser/program dependencies, supported composition features and actual picture acceptance |
| Visual authoring | [Cel sorter](cel-sorter-v1.md), [note editor](music-note-editor-v1.md), [underlay editor](music-underlay-editor-v1.md), [explicit host](editor-host-v1.md) | Native apply, compiler/render checks, supported editing scope and human acceptance; linked language editing and richer intent remain extensions |
| Browser graph inspection | [WASM workbench](https://giodl73-repo.github.io/REEL/assembly/), [source/scope](../browser/assembly/README.md) | This inspects declared graphs and computed locks; it does not resolve media, render or approve assets |
| Music editions, albums and print | [Song editions](NATIVE-SONG-EDITION.md), [albums](NATIVE-ALBUM-MASTER.md), [album video](NATIVE-ALBUM-VIDEO.md), [print](native-print-collections.md) | Source authority, provider/runtime boundaries, supported notation/layout and musical/visual release decisions |
| Existing accounting/custody hosts | [Project workflow](asset-manager-workflow-v1.md), [version diff](asset-version-diff-v1.md), [tier review](asset-tier-review-v1.md), [Tig history host](tig-history.md) | Correct registry/project adapter, all applicable consumers, fresh verified copies and explicit application policy |

## Proposals and separate work

As of this checkpoint these PRs remain open drafts. Their contents are not
implicitly available merely because an adjacent native workflow is on main:

- [#120: local speech/alignment workers](https://github.com/giodl73-repo/REEL/pull/120).
- [#128: alternate bounded lossless assembly work](https://github.com/giodl73-repo/REEL/pull/128).
  Current [managed episode jobs](tig-episode-jobs.md) already exist; this older
  proposal is not the status of all episode execution.
- [#144: managed inventory indexes and publication fencing](https://github.com/giodl73-repo/REEL/pull/144).

Remotion handoff packages exist; Remotion execution, Blender execution and a
universal AI-video provider are planned adapter integrations. Older optional
recognition/engraving/collection scripts also remain; the native print and album
producers do not imply a complete port of every legacy workflow.

## Next useful units

1. Adopt a chosen exact runtime/profile in a producing project, exercising edit,
   compile, render, independent check, review, retention and recovery end to end.
2. Extend unsupported native inputs or editing operations through existing
   contracts, with a small fixture and explicit semantic coverage.
3. Keep generic asset identity, catalogue and lifecycle changes in Tig; keep
   media interpretation and media-to-Tig hosts in REEL.
4. Treat cache-wide enforcement, provider integration and older adapter ports as
   separate work with their own compatibility and operational evidence.

Project adoption reports and large inventories belong in the project asset
system. Historical reviews in [docs/reviews](reviews/) and execution records in
[context](../context/) remain evidence for the exact sources and fixtures they
name; they do not certify another installed runtime or private render.
