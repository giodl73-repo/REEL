# Documentation

Start with [getting started](getting-started.md), then choose a
[use case](use-cases.md). [Architecture](architecture.md) explains ownership,
contracts, engines and the Tig boundary. The root [README](../README.md) provides
the short capability map.

## Learning paths

| Goal | Read in order |
| --- | --- |
| First local plan | [Install](setup/install.md) → [quickstart](getting-started.md) → [production manifest](production-manifest-v0.2.md) |
| Native scene or music video | [Scene authoring](scene-authoring-v1.md) → [scene delivery](scene-delivery-v0.1.md) → [native media comparison](native-media-parity.md) |
| Tig-backed production | [Public integration](tig-public-integration.md) → [native scene compiler](tig-native-scene.md) → [managed jobs](tig-job-runner.md) |
| Episode composition | [Episode planning](tig-episode-planning.md) → [bounded episode execution](tig-episode-jobs.md) → [retained presentations](tig-retained-presentation.md) |
| Visual authoring | [Cel sorter](cel-sorter-v1.md) / [note workshop](music-note-editor-v1.md) / [underlay workshop](music-underlay-editor-v1.md) → [editor host](editor-host-v1.md) |
| Music/print delivery | [Album mastering](NATIVE-ALBUM-MASTER.md) / [album video](NATIVE-ALBUM-VIDEO.md) / [native print collections](native-print-collections.md) |
| Storage operations | [Architecture boundary](architecture.md#reel-and-tig) → [accounting workflow](asset-manager-workflow-v1.md) → [tier review](asset-tier-review-v1.md) |
| Delivery history | [Workflow](use-cases.md#track-a-delivery-over-time) → [history host](tig-history.md) → [delivery observation](tig-delivery-observation.md) |
| Contribution | [Contributor checks](contributing.md) → [capability roadmap](tooling-roadmap.md) → [roles](../.roles/ROLE.md) |

## Which kind of document is this?

- **Task guides:** this index, getting started, use cases, architecture and
  installation orient a new reader to current interfaces.
- **Contract/reference documents:** specify an exact schema, CLI or bounded API.
  A version in the filename often identifies when a feature was introduced;
  it does not mean a current binary implements every later contract. Check
  the command's feature flags, platform, schema ID and runtime pin.
- **Roadmaps and proposals:** describe intended work or source/adoption gaps.
  [The current roadmap](tooling-roadmap.md) names these separately.
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
| Tig catalogue | Generic cross-project asset/storage navigation; see [Tig upstream](https://github.com/giodl73-repo/tig) |

## Source and evidence

[PRODUCT_PLAN](../PRODUCT_PLAN.md) retains the founding product direction;
[context](../context/) retains execution history. Prefer the current task guides
for setup and the referenced contracts for exact behavior. Generated media,
machine profiles, cache inventories and operational receipts stay in the
producing project's asset system, not in these public docs.
