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
reel-scene-authoring resolve-trigger-text <word-evidence.json> <trigger-spec.json> --alignment <new.json> --receipt <new.json>
reel-scene-authoring compile-events <graph.json> <pointer.json> <scene.json> <language> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <alignment-paths.json> <next-lock-id> --output <new.json>
reel-scene-authoring audit-picture <policy.json> <rendered-spans.json> <sample-rate> --output <new.json>
reel-scene-build build <project-root> <build.json> --asset-root <hydrated-cache-root> --output-dir <new-dir>
reel-scene-build emit-changed-only-graph <index.json> --asset-root <hydrated-cache-root> --output <new-graph.json>
reel-scene-build execute-changed-only <index.json> <prior-state.json> --asset-root <hydrated-cache-root> --output-root <new-run-dir>
reel-scene-build execute-episode <project-root> <episode-build.json> <prior-state.json> --asset-root <hydrated-root> --output-root <new-dir-within-hydrated-root>
reel-scene-template compile <catalog.json> <definition.json> <scene.json> <language> <season-bindings.json> <episode-bindings.json> <scene-bindings.json> <alignment-paths.json> <source-text.json> --output-ass <new.ass> --receipt <new.json>
reel-episode-conform build <manifest.json> --input-root <authoring-root> --asset-root <hydrated-media-root> --output-dir <new-dir>
```

`resolve` returns both whole-scene and language-local fingerprints. A Spanish
take rebind changes the Spanish fingerprint only. A season opening rebind
changes episode presentation without marking every narrative scene stale.
`resolve-trigger-text` turns an author-named spoken phrase into a sample-exact
native marker using word evidence from the selected take. The trigger file
contains the exact cue text, its SHA-256, selected take SHA-256, and ordered
phrase names; it contains no cut seconds. Repeated phrases require an explicit
one-based occurrence. The command checks cue, language, take, text hash, phrase
occurrences and word clocks, then writes a native alignment and an input-hash
receipt. Machine word evidence and the resulting markers remain listening and
review holds until their entrances are checked against the actual recording.
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
that points to the catalog, episode, scene, policy, scoped bindings, selected
native-alignment path map and semantic-delivery contract. It verifies the
selected alignment bytes, recompiles the native event spans, and requires the
selected graph phrase clocks to match their exact samples. It resolves the requested language fingerprint,
checks exact active event IDs, calls REEL plan/render/check, groups adjacent
identical asset/crop spans, and writes a private build receipt. Template scenes
must bind the exact compiled ASS layer, source text, compile receipt, and font.
The build validates those bindings before rendering.
For each selected language-local semantic event, the build projects its native
phrase samples through the selected D attachment onto the scene clock. Its
selected picture must cover that entire phrase; declared M/E and external
attachments must overlap it. The exact delivered M/E and VFX assets must agree
with the authored score role, Sonic and VFX bindings. A shifted or omitted
attachment with unchanged media hashes fails this check.
The technical grouping does not prove that different asset hashes look
visually distinct; frame inspection remains a separate review step.

`reel-scene-template compile` verifies the exact template definition against
its catalog hash, every selected language-local native alignment against
the scoped asset binding and selected take, and a selected source-text file
against its language-local scoped binding. The selected source-text file must
agree exactly with the scene content on title, chapter number, poem lines,
cue scope, and stanza breaks; its receipt preserves the text review state.
This check binds the invocation to its selected source authority, while actual
human review of that authority remains a separate decision. Scene content
provides language-local text and cue/semantic-marker IDs; the template owns
canvas, panel, font, colors, and chapter duration.
The resulting editable ASS layer shows the complete poem from its first frame,
advances read-state colors at measured native line entrances, and keeps a
chapter card for its template-defined duration. Its receipt records the exact
template and ASS hashes. REEL scene delivery composites the selected ASS layer
into the picture while retaining the clean picture separately. Its technical
check verifies that the selected overlay changes decoded pixels at the scene
midpoint; editorial inspection must still check text legibility and each
transition.

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
  "alignment_paths": "authoring/scene-002/es/alignment-paths.json",
  "semantic_delivery": "authoring/scene-002/es/semantic-delivery.json",
  "template_receipt": "authoring/scene-002/es/template-receipt.json"
}
```

For multiple scenes, `resolve-language` writes one small file per scene and
language. A `reel.scene-build-index.v1` lists its project root and each node's
build manifest, resolved-language file and selected semantic-delivery file.
`emit-changed-only-graph` recomputes the current language fingerprint and
rejects a stale indexed resolver file. It measures that fingerprint file,
selected alignment files, selected scene job, hydrated job media and the exact
build executable. Unrelated opening or shared binding edits leave scene node
inputs unchanged.
`execute-changed-only` uses REEL's planner for each independent scene-language
node, runs only `rebuild` nodes through the same checked scene builder, writes
REEL result receipts, and advances immutable state snapshots. An unchanged
node is reused only after REEL verifies its prior output bytes. The run uses a
new output directory and leaves prior state intact. `execute-episode` runs the
scene nodes, fills exact scene master, build-receipt and delivery-receipt hashes
from the verified final state, then calls generic episode conform. Its
`reel.episode-build.v1` file names a scene build index and a conform template
relative to the project root. The conform template is a normal
`reel.episode-conform.v1` document except each scene segment names
`scene_node_id` and the exact `delivery_job` instead of precomputed `master`,
`source_receipt` and `delivery_receipt` references. Presentation segments keep
their selected hash-bound references. The output directory must be new and
inside the hydrated asset root so rebuilt and reused scene outputs can be
named by root-relative hash references. The generated conform and final scene
state are retained with the lossless episode master. Presentation rendering
and creative approval remain separate gates.

## Current execution boundary

V1 resolution checks contract identity and scope; event and template compilation
verify supplied local alignment and source files. The scene build renders a
selected semantic graph and optional ASS presentation layer, but it does not
hydrate every selected asset, select a REEL graph revision, or infer creative
approval. Changed-only execution covers ready scene-language nodes and can
conform them with selected presentation masters. A selected
`cache://sha256/` binding must still pass the consumer's hydration and authority
checks. Generic episode conform builds a selected lossless master; presentation
segment creation and whole-episode creative review remain separate.
Evidence-only external layers still require a rendered VFX implementation or
an explicit review disposition before they can establish visible VFX delivery.
