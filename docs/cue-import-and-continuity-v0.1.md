# Cue import and shared continuity

## Deterministic SRT cue import

```powershell
reel cue-import-srt timed-manifest.yaml captions.es.srt `
  --speaker bertica-narrator --source-ref selected-range `
  --output upgraded-with-cues.yaml --format json
```

Use `--mapping mapping.yaml` instead of global speaker/source arguments for a
multi-speaker sequence. A mapping assigns every SRT index a stable cue ID,
speaker, source references, and optional pause policy. The importer rejects
overlapping cues, non-contiguous indexes, unknown speakers/sources, cues beyond
the work duration, and cues that overlap no shot. It writes a new manifest
atomically and records the input manifest, SRT, mapping hashes, tool version, and
transformation lineage.

Existing protected pauses survive only when the mapping deliberately preserves
the referenced cue ID. The sanitized fixture imports four poem captions, a
1.5-second protected threshold, and two prose captions; `caption-export`
reproduces its SRT byte-for-byte.

## Shared continuity registry

`reel.continuity.v0.1` stores recurring entity observations and local reference
policies once. Validate one with:

```powershell
reel continuity-validate continuity.yaml --output json
```

A v0.2 scene cites it through the existing extensible `continuity` mapping:

```yaml
continuity:
  external_registry:
    path: ../continuity/bertica-v1.yaml
    version: "1"
    sha256: <expected SHA-256>
    entity_ids: [herrera, bertha-maria, moro]
  entities:
    - id: herrera
      age_at_scene: young-adult
      observations: [Wears a pale travel shirt in this scene.]
      clothing: pale travel shirt
      condition: arriving from the road
```

Scene entities are overlays: age, observations, clothing, condition, confidence,
and other scene-local fields remain local. Registry hashes and versions are
verified during ordinary manifest validation and provider packaging.

Provider packages resolve approved shared observations but serialize neither an
external registry's `local_path` nor a scene's forbidden private path. Requested
assets still require `provider_transfer: approved` and a nonempty approval
reference.
