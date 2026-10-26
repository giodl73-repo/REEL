# Compile a native scene from Tig

`reel-tig-scene compile-native` captures the explicit committed source profile
and resolves the scene's current `authored.binding_ids` map. Each value is exactly
`{"id":"opaque-asset-id"}`. The parent file need not change when a leaf changes.
The parent definition and all primary definitions are explicitly admitted and
pinned by the existing source profile. The native request distinguishes the scene
definition from permitted primary definitions; scene/control/delivery definitions
must not be admitted as primary types. Definition admission is trusted project
policy, not execution of every JSON Schema or Tig semantic rule.

```text
captured Git commit
  scene.tig --binding_ids--> cel.tig / cue.tig / alignment.tig
      |                         |
      |                  selected bytes + current status
      |                         |
      +---- selected scene -----+---- fresh scoped bindings
      +---- named controls -----+---- measured alignment clocks
                                         |
                                current assembly projection
                                         |
                                 existing REEL compiler
                                         |
                                ID + hash + size in job
                                         |
                            existing Tig guarded input provider
```

The source profile still supplies the repository, full commit, namespace scopes,
exact definition pins/selectors and existing cache roots. The compile request is
small native REEL policy. For example:

```json
{
  "schema": "reel.tig-native-scene.experimental-1",
  "scene_id": "scenes/dawn",
  "scene_definition": "types/native-scene",
  "primary_definitions": ["types/picture", "types/take", "types/alignment"],
  "language": "es",
  "controls": {
    "catalog": "catalog.json",
    "episode": "episode.json",
    "policy": "policy.json",
    "profile": "profile.json"
  },
  "delivery_id": "dawn-es-private",
  "delivery_title": "Private scene build"
}
```

```powershell
reel-tig-scene --profile .local/tig.json compile-native .local/dawn-es.json .local/build-dawn-es
reel-tig-scene --profile .local/tig.json plan .local/build-dawn-es/delivery/job.json
reel-tig-scene --profile .local/tig.json render .local/build-dawn-es/delivery/job.json renders/new-dawn-es
```

The selected parent object is native `scene.json`, not rendered video. Its
`authored.scene_id` must match that scene. Control names select exact named streams
on the same parent revision; they are never executed as scripts or interpreted as
paths. Retained `bindings.json`, old assembly graphs and old alignment-path lists
are not current dependency authorities. The adapter generates scene bindings from
current leaves, empty enclosing scopes, the requested language's alignment paths
and a current assembly projection. Version history stays in the Tig records/Git;
this derived assembly is a current snapshot, not an append-only history import.

Native scene validation still resolves the entire declared language matrix;
this version does not infer partially completed variants. Each declared leaf must
have an admitted selected/named payload and one of native REEL's eligible
selection states. A draft leaf refuses before reading cached bytes. Replacing a
take without its matching alignment refuses; historical captured views retain
their own payloads and status. `Source::resolve_asset(id)` is the metadata-only
library operation for deriving an exact `AssetRef`; it does not read media or
grant production authority.

The new output directory contains bounded private metadata only. Original scene
bytes, including holds and source text, are preserved. Controls are at most 4 MiB
each, with 16 MiB aggregate metadata and 4,096 bindings. The 64 KiB request is read
with a bound and rejects duplicate/unknown fields. Profile `asset_paths` is derived
as `asset-id`. Original control pins and the captured parent record/checkpoint are
recorded in `native-source.json`; this is private provenance, not a public receipt.
Optional presentation receipts use an explicit `controls.template_receipt` stream;
the compiler still compares it to the fresh selected receipt binding. Sonic
placement evidence uses protected exact input pins during compilation. Rendering
subsequently prepares the whole job independently.

The adapter creates no media cache, copies no primary media, runs no transfers,
and changes no runtime binding. Compilation is not production approval or asset
registration. Errors may leave a partial new metadata directory for diagnosis;
it is not a completed build. `complete_semantic_coverage` remains false. Full
schema/variant rules, source/job recovery, custody/eviction authority, family
migration, rendered parity and explicit production cutover remain open.
