# Imported scene source proof (development)

This route preserves granular historical graph scenes and original native jobs.
It is distinct from an authored scene build. It does not make a complete movie
an opaque presentation unit, infer creative approval, or authorize publication.

The manifest pins the original selected graph, selected pointer, frozen source
capture, semantic delivery, native job and production file by SHA-256 and bytes.
The source capture has schema `reel.imported-scene-source-capture.v1` and pins:

- `episode_id`, `scene_id`, and `source_graph.sha256`;
- `selected_pointer_sha256`;
- `selected_delivery_jobs.<language>.sha256`;
- `selected_semantic_deliveries.<language>.sha256`;
- `event_cue_ids.<event_id>` for each scoped event.

These pins must come from independently retained original selection, job,
contract and event records. Copying candidate hashes into a new capture does
not establish historical provenance. Existing capture schemas cannot be
renamed to this schema without reconciling that original evidence.

Version 1 requires the exact original graph and job. It checks the selected
pointer against the graph lock, requires a single scene node without dependency
inputs, and verifies semantic event/attachment closure. Native planning checks
D cue identity, source trims, source phrase clocks and picture coverage, plus
M/E/VFX phrase intersection. The independently frozen exact job hash preserves
the original compiled offsets, gains, fades and attachment sources; the exact
semantic hash preserves event-to-attachment associations. Output verification
fully checks the native rendered scene. Episode conform rechecks these inputs
and outputs when explicitly given `imported_source_manifest`.

Changed title pictures or other successors need their own explicit source
transformation proof. They must fail this unchanged-import route.

## Remaining validation before production use

Identity and phrase arithmetic fixtures and a real FFmpeg scene/conform fixture
exercise D/M/E, two cels, an overlay spanning both, and stale job/binding/output
rejections. Independent source capture reconciliation and final bounded review
are still required for each historical source. No production runtime is
selected from this development branch yet.
