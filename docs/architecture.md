# Architecture and responsibility

REEL turns declarative media controls into explicit jobs and independently
checked outputs. Its reusable boundary is the documented contract or adapter,
with exact input identities and an admitted runtime. Crate versions alone do
not identify every schema, producer or compatibility promise.

```text
Producing project                     REEL
-----------------                     ----
Text, scenes, timing, definitions ---> validate / resolve / compile
Pictures and recordings ------------> explicit input-provider port
Profiles, budgets, runtime pins -----> bounded job execution
                                         |
                              external FFmpeg / browser
                                         |
                              independent frame/sample checks
                                         |
Review + selection + delivery <------- candidate + private evidence
```

## Four kinds of data

| Kind | What it means | Owner |
| --- | --- | --- |
| Authored controls | Source text, scene order, cue/event intent, templates, bindings and delivery choices | Producing project |
| Resolved inputs | Exact selected asset IDs, byte hashes/sizes and admitted definition objects from one captured view | Project declarations resolved by the selected provider |
| Compiled job | Derived timing, bindings, recipes, runtime requirements and bounded execution parameters | REEL compiler; retained by the project/job system |
| Produced output and evidence | Media, checks, lineage and candidate record proposals | Project asset system, with technical evidence from REEL |

A changed cel or cue can change a compiled recipe while the parent scene's
authoring ID stays stable. An unused language variant may leave another
language's consumed recipe unchanged. The exact behavior depends on the
[native binding and consumption contract](tig-native-scene.md), not filename
guessing. Existing frozen contexts keep their admitted compiler/runtime;
recompiling current source creates a new context rather than rewriting history.

## Contracts and compilers

The original [production manifest](production-manifest-v0.2.md) is YAML with
validation, timing conform, export planning and legacy preview/render commands.
Native [scene authoring](scene-authoring-v1.md) separates semantic cue/event
intent from scoped bindings, measured alignment, presentation templates and
delivery policy. The native compiler derives exact frame/sample jobs from those
controls. Native [episode planning](tig-episode-planning.md) composes declared
scenes and presentations in order using their checked component contracts.

These interfaces coexist. They are not interchangeable serializations of one
universal `scene.json`. An adapter must account for meaning, input identities and
current proof requirements before claiming equivalent output.

## REEL and Tig

[Tig](https://github.com/giodl73-repo/tig) is the generic asset layer. Its asset
definitions, instances, byte identities, Git snapshots, storage providers and
lifecycle services also serve projects that never render media.

REEL's [optional Tig adapters](tig-public-integration.md) capture a specified Git
view, admit exact definitions, resolve logical asset IDs and hold verified bytes
through native rendering/checking. Publication/history adapters connect checked
results to explicitly configured Tig stores and record proposals. Tig source is
[pinned and vendored](../vendor/README.md); REEL does not fork its generic engine.

REEL owns media semantics: how cues, cels, scores, languages, frames and samples
become a delivery. The project owns which definitions, variants, runtimes,
approvals and delivery targets are authorized. Google Drive, YouTube or a drive
letter are project choices, not built-in asset identities.

The existing `reel-assets` crate also supplies project-profile-based accounting,
editors and custody adapters used by earlier workflows and current hosts. Its
[version registry/tier contracts](asset-manager-workflow-v1.md) are distinct from
a Tig project view. Use the documented adapter and explicit coverage boundaries
when connecting them. Verified input availability, a rendered candidate and
authority to evict bytes are different claims.

## Engines and adapters

| Component | Responsibility |
| --- | --- |
| Main `reel` CLI | Manifest planning, conform, previews, checks and production utilities |
| Native media engines in `src/` | Scenes, presentations, episodes, albums and independent media comparison |
| `reel-assembly` | Semantic graph, event, dependency and lock contracts |
| `reel-music` | Native music interpretation, edit proposals and interchange contracts |
| `reel-print` | Bounded print packets from pinned text, score SVG and retained PDF pages |
| `reel-assets` | Profile-based accounting/custody services and native editor adapters |
| `src/tig_*.rs` and optional binaries | Thin media-to-Tig integration, managed jobs, registration and history hosts |
| FFmpeg/FFprobe | External media processing and independent probes/decodes |
| Chromium-compatible browser | Explicit frame-clock rendering for supported local HTML/SVG/CSS/JS sources |

Remotion package generation and Blender/AI-video adapter declarations describe
handoff boundaries. They do not install those runtimes or constitute implemented
production execution. See `reel adapters --output json` and the
[capability roadmap](tooling-roadmap.md).

## Authoring surfaces

Cel, note and underlay editors export a source-locked session and a draft edit
request. Native apply validates that request and writes a new candidate. The
[Rust editor host](editor-host-v1.md) serves one explicitly chosen export on a
private loopback URL. It is launched by the user, not by repository startup.

The [browser/WASM workbench](https://giodl73-repo.github.io/REEL/assembly/) runs
semantic graph inspection in the browser without a native media cache. It is a
separate surface from native editor hosting, browser-based frame rendering and
Tig's multi-project asset catalogue.

## Execution and trust boundaries

Planning/metadata inspection, media verification, execution, retention, creative
selection and delivery are separate steps. Managed jobs require declared stores,
runtime inputs, capacity limits and ownership. Recovery can finish a prepared
handoff without rerendering; failure preserves evidence and live obligations
until the matching recovery/retirement contract permits their release.

Managed native runtime guards, strict path-based Tig jobs and contained browser
workers currently rely on Windows protection. Unsupported protection refuses
rather than silently falling back. Legacy Windows animatic transport uses WSL;
native media APIs use explicit native program paths. Installation must match the
chosen route.

Machine profiles and full operational evidence stay private. Human review and
rights decisions remain explicit project authority. Tests establish bounded
technical behavior, not consent, taste, public release or complete cache-wide
custody. See [review roles](../.roles/ROLE.md) and
[principles/pitfalls](../.pitfall/PITFALL.md).
