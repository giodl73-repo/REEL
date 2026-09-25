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
reel-scene-authoring resolve-episode-presentation <catalog.json> <episode.json> <season-bindings.json> <episode-bindings.json> --output <new.json>
reel-scene-authoring audit-picture <policy.json> <rendered-spans.json> <sample-rate> --output <new.json>
```

`resolve` returns both whole-scene and language-local fingerprints. A Spanish
take rebind changes the Spanish fingerprint only. A season opening rebind
changes episode presentation without marking every narrative scene stale.
Adjacent events with the same visible composition are grouped for the policy
maximum. Preferred cadence produces a hint; exceeding the hard maximum fails.
A montage may name an explicit different policy. These fingerprints can become
direct inputs to REEL's existing `changed-only-plan` graph.

## Current execution boundary

V1 resolution checks contract identity and scope; it does **not** verify that
the cache contains each declared byte, derive phrase clocks, select a REEL
graph revision, render templates, execute changed-only actions, conform an
episode, or infer creative approval. A selected `cache://sha256/` binding must
still pass the consumer's hydration and authority checks. The existing REEL
semantic assembly and scene delivery commands remain the render path until a
generic executor connects these contracts to them. The episode checker consumes
an existing master; REEL currently has no episode renderer.
