# Documentation

The root [README](../README.md) is the executive overview. Start here for a
functional area, follow an illustrated example, then open the exact task or
contract when you need implementation detail.

## Illustrated guides

| Functionality | Start with | Example you will see |
| --- | --- | --- |
| Episodes and languages | [Scenes, cues and language editions](guides/episodes-and-languages.md) | Two equivalent phrases driving different English/Spanish picture clocks |
| Animation and picture | [Cels and semantic effects](guides/animation-and-effects.md) | One door-opening event changing a cel, Sonic cue and visual layer |
| Voice and sound | [Takes, alignment and sound layers](guides/voice-and-sound.md) | Separate dialogue/music/effects buses and the scope of an audio revision |
| Music | [Songs, scores, albums and journeys](guides/music-and-albums.md) | Recording versus editable model; three complete tracks joined into an album |
| Comics | [Pages and editable layers](guides/comic-layers.md) | Correct a balloon while keeping clean artwork; active workflow development scope |
| Languages and publishing | [Editions and delivery history](guides/editions-and-delivery.md) | Keep a revised movie, its captions and target evidence matched |
| Production engineering | [git-asset and revisions](guides/assets-and-revisions.md) | Replace one cel while retaining a frozen earlier source/job |

Each walkthrough links to its relevant contracts. The [capability map](capabilities.md)
lists the wider toolset; [use cases](use-cases.md) describe task sequences; the
[reference index](reference-index.md) is the full contract catalogue.

For proposed work beyond the current delivery routes, see the
[media format expansion plan](media-format-expansion.md), starting with reusable
spoken-content modules and audience/language editions. Its
[phased PR sequence](media-expansion-pr-plan.md) defines the implementation gates.

## Learning paths

| Goal | Read in order |
| --- | --- |
| Understand REEL's scope | [Capabilities](capabilities.md) → [architecture](architecture.md) → [development status](development-status.md) |
| First local plan | [Install](setup/install.md) → [quickstart](getting-started.md) → [production manifest](production-manifest-v0.2.md) |
| Native scene or music video | [Illustrated episode model](guides/episodes-and-languages.md) → [scene authoring](scene-authoring-v1.md) → [scene delivery](scene-delivery-v0.1.md) |
| Asset-backed production | [git-asset and revisions](guides/assets-and-revisions.md) → [native integration](tig-public-integration.md) → [managed jobs](tig-job-runner.md) |
| Episode composition | [Episode planning](tig-episode-planning.md) → [bounded episode execution](tig-episode-jobs.md) → [retained presentations](tig-retained-presentation.md) |
| Visual authoring | [Cel sorter](cel-sorter-v1.md) / [note workshop](music-note-editor-v1.md) / [underlay workshop](music-underlay-editor-v1.md) → [editor host](editor-host-v1.md) |
| Music/print delivery | [Album mastering](NATIVE-ALBUM-MASTER.md) / [album video](NATIVE-ALBUM-VIDEO.md) / [native print collections](native-print-collections.md) |
| Storage operations | [Architecture boundary](architecture.md#reel-and-git-asset) → [accounting workflow](asset-manager-workflow-v1.md) → [tier review](asset-tier-review-v1.md) |
| Delivery history | [Workflow](use-cases.md#track-a-delivery-over-time) → [history host](tig-history.md) → [delivery observation](tig-delivery-observation.md) |
| Contribution | [Contributor checks](contributing.md) → [capability roadmap](tooling-roadmap.md) → [roles](../.roles/ROLE.md) |

## Which kind of document is this?

- **Illustrated walkthroughs:** `guides/` explains the media/source model through
  diagrams and clearly labeled examples. Illustrative timing is not recorded
  evidence or a ready-to-run production request.
- **Task guides:** getting started, use cases, architecture and installation
  explain the next operational steps and ownership boundaries.
- **Contract/reference documents:** specify an exact schema, CLI or bounded API.
  A version in the filename often identifies when a feature was introduced;
  it does not mean a current binary implements every later contract. Check
  the command's feature flags, platform, schema ID and runtime pin.
- **Development references:** [development status](development-status.md)
  points to active workflow work. The [dated roadmap](tooling-roadmap.md) and
  proposals preserve intended work and source/adoption gaps at their checkpoints.
- **Reviews, waves and handoffs:** record evidence for a particular source tree,
  fixture or decision. They are historical records, not automatic certification
  of a different runtime or a private project.

Use the [complete reference index](reference-index.md) to find all top-level
guides by topic. Existing contract filenames and links remain stable.

## Browser surfaces

| Surface | Purpose |
| --- | --- |
| [Hosted semantic workbench](https://giodl73-repo.github.io/REEL/assembly/) | Inspect native graph dependencies/events and lock observations in WASM; no media cache or rendering |
| [Native editor host](editor-host-v1.md) | Explicitly serve one cel/note/underlay export locally; native apply remains separate |
| [Frame-clock browser renderer](deterministic-browser-compositions-v0.3.22.md) and [managed native worker](tig-browser-native-runtime.md) | Produce actual frames from admitted local HTML/SVG/CSS/JS sources |
| Asset-management tooling | Asset relationships, versions and storage policy; see [git-asset](https://github.com/giodl73-repo/git-asset) |

## Source and evidence

[PRODUCT_PLAN](../PRODUCT_PLAN.md) retains the founding product direction;
[context](../context/) retains execution history. Prefer the current task guides
for setup and the referenced contracts for exact behavior. Generated media,
machine profiles, cache inventories and operational receipts stay in the
producing project's asset system, not in these public docs.

The [illustrated documentation role review](reviews/2026-11-20-illustrated-documentation-role-review.md)
records the README/guide review, technical claim corrections and review boundaries.
