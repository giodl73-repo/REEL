# Reuse a delivered output in a programme

`tig_retained_presentation` binds a complete selected delivered file to one
presentation occurrence. It preserves the delivered bytes; it does not pretend
the movie is a primary recording or attest its original upstream producer.

The source record has these authored fields:

```json
{
  "source": { "id": "delivery/film-one" },
  "language": "en",
  "episode_id": "programme-one",
  "role": "film-one",
  "adoption_slot": "adoption-manifest"
}
```

Its named stream retains the original `reel.presentation-adopt.v1` manifest with
`output_mode: retain-source`. The definition declares those five authored
properties. The source ID names an explicitly admitted delivered-output type.
The selected source hash/length must equal the adoption's source subject.
Language and occurrence scope must match the adoption. No movie bytes are read
by metadata lowering. The source movie may be shared across language editions;
the occurrence language does not relabel or translate the movie.

```text
source.tig -- source ID --> delivered-output.tig --> selected movie bytes
    |
    +-- exact adoption metadata
    |
    +-- native parent occurrence --> same selected movie bytes

Production graph: authored control container only
Composition resources: selected delivered movie + occurrence metadata
```

Use an explicit native source profile and request:

```json
{
  "schema": "reel.tig-retained-presentation.experimental-1",
  "root_id": "programme/film-one/en",
  "root_definition": "types/retained-presentation",
  "source_definitions": ["types/delivered-output"]
}
```

`reel-tig-scene --profile PROFILE plan-retained REQUEST` resolves the source,
scope and bounded exact metadata. It reports `executable: false`, no media
verification and no primary-reference claim. `freeze_context`, `restore_context`
and `recipe` use the existing captured namespace and Production-context APIs.
The output's selected bytes enter the composition recipe descriptor and resource
obligations, never a primary node. Metadata replay works with Git unavailable.
A changed delivered subject refuses the stale adoption; a new adoption must
explicitly bind the newly selected version while preserving the old version.

In an episode job, supply this request in the ordinary `presentations` entry,
with its segment ID, output ID and output definition; omit browser execution.
The output record's `presentation.id` names the retained source root. Its
language, episode, occurrence role and selected bytes must agree. Source,
output and control definition roles are distinct. The worker freezes both
resource and runtime obligations, adopts its per-source fence, holds source and
occurrence byte guards, and freshly checks scoped adoption, media geometry,
duration, decoded content and timestamps. Scratch admission covers its bounded
temporary reference copy, which is removed before parent use.

The sealed declaration identifies the retained source and reports
`primary_reference_verified: false`; it supplies no fabricated RGB comparison or
upstream producer claim. Component-verification results set
`component_primary_references_verified` false when any component uses retention,
while separately reporting current-reference verification. Existing primary
scene/still/browser checks keep their stronger primary-reference meaning.

Conform accepts compressed media only after a checked retain-source adoption or
sealed retained reference. It normalizes video and audio to the selected
lossless intermediate format, verifies decoded equivalence and then uses the
ordinary ordered assembly checks. Unverified compressed segments remain
ineligible. This may encode an assembly intermediate; it does not rerender the
film from its original cels or cues or change the delivered source movie.

Technical reuse is separate from source freshness, rights, human review,
selection, publication, custody and eviction. A project may require a separate
upstream-currency gate. The frozen old version remains usable as an explicitly
retained edition; metadata import alone does not establish that its producer is
current.
