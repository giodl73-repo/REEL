# Failed episode context retention

The generic runner retains failure diagnostics before returning an error. The
episode coordinator now attempts context handoff against the authoritative
retained result on that path too. An unretained failure still refuses handoff and
preserves its claims. Error text and a missing success report never establish a
successful render.

Failed results retain original request/evidence objects, the declared input
closure and every present component context before releasing redundant claims.
The worker can stop before creating a component, between prepare/adopt, or after
adoption. A pending component requires its exact initial worker prepare
acknowledgment. Parent contexts must match the admitted recipe/source/control
scope; other component roots must be in the declared input closure. Budgets and
independent metadata readback still apply. Adoption under the root precedes
component abort/withdrawal, and the packet preserves the failed outcome.

Synthetic metadata tests exercise absent, prepared and adopted scene/parent
combinations, including interruption during failed-result handoff and store
reopening. They check original diagnostics and every component entry survive in
the adopted packet, all released claims have the expected revisions, repeat
cleanup has no journal effect, and a foreign component prepare leaves everything
live. The full CLI fixture also forces an actual contained worker failure after
component adoption using an insufficient reference-staging budget, before any
outputs are written. It checks the failure exit/result, retained diagnostic
hashes, all four temporary scene claims and idempotent metadata-only finish.
The initial tiny-output-ceiling experiment correctly refused retirement and left
staging/source claims live; this remains an explicit refusal case.

Local qualification passes 228 enabled library tests, all five focused handoff
tests, the expanded CLI fixture in 187.15 seconds, strict locked
all-target/all-feature Clippy and formatting. Hosted qualification remains open.

Producer-applied provenance review protects failed evidence and separates claim
release from byte eviction. Editor/sound review leaves failed output ineligible;
cleanup grants no decoded-content, creative or delivery approval. These are
checklist findings, not independent approval. Per-source fences allow unrelated
jobs to proceed; this adds no cache-wide render lock or daemon.

An interruption before runner result adoption, generation/text freshness,
output-record/Git handoff, actual project acceptance and runtime cutover remain
separate work. No production bindings, cache topology or payloads are changed by
this feature's qualification.
