# Register independent retained component outputs

`reel-tig-register --conform-components PRIVATE_PROFILE NEW_DIRECTORY` maps
explicit native episode conform segments to independent Tig instances. It reuses
the existing protected cache reader and exact `reel.episode-conform.v1` parser.
It reads conform metadata only. Selected movies and original production evidence
need not be locally present; registration does not verify those payloads.

The private JSON profile has schema
`reel.tig-conform-components-registration.experimental-1` and these fields:

- `episode_id`: exact case-sensitive native episode identity.
- `byte_source`: existing Tig byte-provider enrollment and reader limits.
- `conforms`: language to exact `{sha256, bytes}` conform objects.
- `entries`: explicit mapping names to the entry structure below.
- `metadata`: optional project provenance; cannot replace generated provenance.
- `max_output_bytes`: candidate record budget, at most 16 MiB.

Each entry supplies `id`, portable one-component `.tig` `filename`, `language`,
`segment_id`, exact `kind` (`scene` or `episode-presentation`), `definition` and
`authored`. Definition files are explicit absolute `{path, sha256, bytes}` pairs
under `definition.definition` and `definition.authored_schema`. Each is bounded
to 64 KiB. `authored` contains project fields such as a desired scene reference.
The tool supplies `language`, `source_segment_id` and `component_kind`; callers
cannot override them. Definitions must declare those fields.

There are at most 32 conforms and 128 explicit mappings. Each metadata object is
limited to 4 MiB, with a 16 MiB aggregate. Profile JSON is at most 64 KiB. Duplicate
JSON keys, unknown fields, conflicting scopes/kinds, duplicate segment identities,
duplicate record IDs, case-insensitive filename collisions, contradictory digest
sizes and changed definition pins refuse registration. Unmapped segment pins and
conform control pins are checked too. Every candidate binds to its exact definition
and authored-schema pin before any output directory is created. This binding checks
declared fields; it does not execute project JSON Schema or semantic rules. Run the
admitted schema/graph validators before committing project records.

Candidates are written under `NEW_DIRECTORY/inputs/<filename>` using the shared
registration writer. The staging directory name does not classify the asset as a
primary input: these selected masters are component **outputs**. The caller chooses
the final repository paths and must supply an output definition/graph adapter.
The directory must be new; retries cannot overwrite it. Failed writes may leave
partial candidate files; no Git proposal or adoption is performed.

Each record selects the exact segment master and retains the original conform,
source receipt and available job/delivery/semantic/adoption/text/import pins as
named evidence streams. Original absolute paths are not copied into generated
records. The exact conform retains original path provenance in asset storage.
Status remains `registered-native-component`, with `production_state: unclassified`
and `new_approval: false`. A desired scene link is not a captured producer assertion.
No current production stamp, decoded parity, creative approval, independent custody,
eviction authority, rendering or media transfer follows from this operation.

## Consuming retained presentation outputs

The native Rust consumer can use `episode_conform::PresentationFiles` to borrow
two existing `scene_delivery::InputFiles` providers per presentation: one for its
exact controls/source, one for its selected outputs. Output aliases are
`master.mkv` and `receipt.json`; hashes and lengths must match the conform pins.
`build_with_presentation_files` covers adopted presentations alongside existing
local scene consumption. `build_with_segment_files` also requires the exact
native scene input/output maps, providing the combined path for an admitted job.
Provider maps must cover their respective segment kinds exactly.

Tig callers can prepare explicitly bound files through `Source::prepare_files`
and retain the resulting `Inputs` owners; native scene jobs still require the
complete `prepare(job)` admission. A source `Inputs` can be wrapped with the
already admitted `BoundPrograms::with_inputs` for absolute media tools. Neither
reading outputs nor adopting a presentation establishes producer currency for
the whole episode, selection, custody or retirement permission.

The episode control JSON and adoption manifest remain bounded local compiler
products. The original presentation media and outputs may be absent from that
tree. Direct borrowed presentations receive the same full decoded-source and
clock recheck, even when local verification reuse was requested; no implicit
verification cache is written in the asset root. Episode-level tool injection
and the end-to-end managed episode executor remain separate required work.
See [the fixture and review](reviews/2026-10-28-cache-presentation-adoption.md).
