# Scoped scene authoring (V1 contract)

`reel-assembly::scene_authoring` owns the reusable scene, template, score,
asset-binding and policy contracts. The `reel-scene-authoring` binary resolves
their exact selected inputs and audits the rendered picture clock.

## Ownership

| Scope | Owns | Rebind effect |
| --- | --- | --- |
| Template catalog | Versioned episode, opening, poem, chapter-title, credits and other definitions | Only invocations of that template |
| Season bindings | Shared opening or other season assets | Episode presentation closure; scene closure only if a scene explicitly uses the binding |
| Episode authoring | Ordered scene IDs, presentation invocations, poem-derived score roles and default policy | Episode presentation or scenes that use a changed score role |
| Episode bindings | Exact selected arrangements, credits and other episode assets | Only actual consumers |
| Scene authoring | Source scope, bilingual cue identities, semantic events, presentation content, continuity and explicit policy override | That scene |
| Scene bindings | Selected language takes, phrase alignments, pictures, Sonic and VFX with cache hashes and byte counts | Only consuming scene language lanes |
| Scene policy | Preferred cadence, hard unchanged-composition maximum and semantic-cut requirement | Scenes naming that policy |

The scene invocation says `template_id`, role and content such as poem ID,
canonical line-to-cue mapping or chapter number/title. Font, panel geometry,
color, fixed presentation duration and layout belong to the template definition.
The selected asset binding belongs to its scope, never to the template layout.
The canonical source document hash and a shared episode graph snapshot belong
in their owner registers. A scene names `source_authority_id`, source scope IDs
and, during migration, a shared evidence ID plus its own event IDs. It does
not copy a whole-episode graph hash into every scene file.
Poem line highlighting follows language-local measured cue/phrase events.
Ready cues name their selected narration slot and take/alignment bindings;
ready events name a selected picture slot and binding. The Rust
`compile_native_event_spans` function converts measured semantic markers to
sample-exact, gapless event spans independently for each language and rejects
unmeasured or out-of-order entrances. It does not author an arbitrary time cut.
Historical backports use `authoring_state: imported-evidence` and keep exact
legacy references. The resolver rejects them until a producer has completed
the authoring fields and explicitly advanced the state to
`ready-for-private-build`; importing a prior render never does that by itself.

An episode score palette maps a scene role to a theme, its source poem, an
arrangement ID and an episode asset binding. Scene events refer to the role or
declare silence/held. The resolver requires the exact selected binding only
when a scene uses that role. Score placement remains source-event based; gain,
ducking and listening checks belong to subsequent delivery contracts.

## Commands

```text
reel-scene-authoring resolve <catalog.json> <episode.json> <scene.json> <policy.json> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> --output <new.json>
reel-scene-authoring resolve-language <catalog.json> <episode.json> <scene.json> <policy.json> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <language> --output <new.json>
reel-scene-authoring resolve-episode-presentation <catalog.json> <episode.json> <season-bindings.json> <episode-bindings.json> --output <new.json>
reel-scene-authoring compile-events <graph.json> <pointer.json> <scene.json> <language> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <alignment-paths.json> <next-lock-id> --output <new.json>
reel-scene-authoring audit-picture <policy.json> <rendered-spans.json> <sample-rate> --output <new.json>
reel-scene-build build <project-root> <build.json> --asset-root <hydrated-cache-root> --output-dir <new-dir>
reel-scene-build emit-changed-only-graph <index.json> --output <new-graph.json>
```

`resolve` returns both whole-scene and language-local fingerprints. A Spanish
take rebind changes the Spanish fingerprint only. A season opening rebind
changes episode presentation without marking every narrative scene stale.
Adjacent events with the same visible composition are grouped for the policy
maximum. Preferred cadence produces a hint; exceeding the hard maximum fails.
A montage may name an explicit different policy. These fingerprints can become
direct inputs to REEL's existing `changed-only-plan` graph.
`compile-events` reads a cue-ID-to-relative-file-path JSON map, verifies every
alignment file's SHA-256 and byte count against its selected scoped binding,
derives native event spans, and emits a request for the existing
`reel semantic-assembly-bind-events` command. REEL checks the request against
the selected graph and rejects stale take or picture slots. A correction names
`supersedes_event_id` in the scene event and receives a new immutable lock.
The independent scene build command reads one `reel.scene-build.v1` manifest
that points to the catalog, episode, scene, policy, scoped bindings and selected
semantic-delivery contract. It resolves the requested language fingerprint,
checks exact active event IDs, calls REEL plan/render/check, groups adjacent
identical asset/crop spans, and writes a private build receipt. It refuses a
scene with template presentation until a real editable layer render is bound.
The technical grouping does not prove that different asset hashes look
visually distinct; frame inspection remains a separate review step.

The build manifest contains repository-root relative paths and no copied asset
hashes:

```json
{
  "schema": "reel.scene-build.v1",
  "scene_id": "scene-002",
  "language": "es",
  "catalog": "authoring/template-catalog.json",
  "episode": "authoring/episode.json",
  "scene": "authoring/scene-002/scene.json",
  "policy": "authoring/policy-narrative.json",
  "season_bindings": "authoring/season-bindings.json",
  "episode_bindings": "authoring/episode-bindings.json",
  "scene_bindings": "authoring/scene-002/bindings.json",
  "semantic_delivery": "authoring/scene-002/es/semantic-delivery.json"
}
```

For multiple scenes, `resolve-language` writes one small file per scene and
language. A `reel.scene-build-index.v1` lists each node ID, its resolved-language
file and its selected semantic-delivery file. `emit-changed-only-graph` measures
those files plus the exact build executable and emits REEL's existing
`reel.changed-only-graph.v0.1`. Run `reel changed-only-plan` with that graph and
the prior state; record successful scene outputs through REEL's existing
result-receipt and state-advance commands. There are no implicit dependencies
between scene-language nodes. Episode presentation and final conform will be
additional nodes once their executors are implemented.

## Current execution boundary

V1 resolution checks contract identity and scope; event compilation verifies
the supplied local alignment files but does **not** hydrate every selected
asset, select a REEL graph revision, render templates, execute changed-only actions, conform an
episode, or infer creative approval. A selected `cache://sha256/` binding must
still pass the consumer's hydration and authority checks. The existing REEL
semantic assembly and scene delivery commands remain the render path until a
generic executor connects these contracts to them. The episode checker consumes
an existing master; REEL currently has no episode renderer.
