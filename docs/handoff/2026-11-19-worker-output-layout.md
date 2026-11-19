# Owned worker output paths

The owned deterministic worker now separates logical artifact names from physical
paths beneath its packet directory. Production obtains a checked return mapping
from the capability dispatcher. Recovery checks that mapping against the exact
owned manifest and every returned artifact hash/length, including the separate
worker receipt. Current producers keep their original identity mappings and JSON;
this change adds no new executable capability or configuration field.

The common layout check accepts portable nested paths, requires exactly the
declared output names, refuses missing or changed output bytes, case aliases,
file/directory collisions, traversal, reserved Windows components and collisions
with `receipt.json`. It applies the existing staging count/path limits; result
ingress retains its separate byte and native transition limits.

Editable comic sources and issue assembly companions remain in the owned manifest.
They are not promoted to extra returned artifacts. The capability's existing
independent producer replay/inventory check still establishes their exact set and
bytes before import. The common layout check alone cannot authorize companions,
authenticate execution or establish retained ownership.

Four component tests cover nested frame/font paths and exact pins, unsafe paths
and collisions, retained editable companions, missing/retargeted outputs and
unchanged flat-name mappings. Native existing-pool evidence for auxiliary tools
remains qualified at its original source pin; it is not evidence for this later
change or complete movie production.

Validation: 55 default native tests with `--features film`, including all 247
original SDK source pins, pass. Strict all-target Clippy, formatting and
`git diff --check` pass. The four layout tests and strict Clippy were rerun after
the final shared-limit/error-class adjustment. No new native-pool effects or
media renders were required for this bounded metadata/recovery change.

Producer-applied editor and rights/provenance checks: preserving renderer-relative
paths allows frame/font dependencies to remain addressable; output identity and
the worker receipt remain separately pinned. No creative selection, consent,
issue publication or release authority follows from accepting a layout.

Next integration: a bounded movie capability must provide its explicit mapping,
preserve the root delivery receipt under a distinct name, execute the existing
renderer with original pinned auxiliary programs, independently check produced
media and retain/reopen the full result under the original job. None of those
movie lifecycle steps is established by this layout change.
