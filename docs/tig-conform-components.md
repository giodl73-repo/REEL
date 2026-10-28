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
