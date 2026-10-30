# Episode context capture through the shared command

`capture-context` now accepts an explicit episode capture schema alongside the
existing scene schema. The schema chooses one strictly validated request family;
it does not infer a family from a partially matching JSON document. Episode
request role definitions remain distinct. Scene capture keeps its compiled
control files and byte-for-byte readback checks.

Both routes share existing-root validation, admitted-volume checks, conservative
metadata/preparation reservations, cumulative provider contention allowance,
finish/reopen, descriptor/requirements/archive equality and atomic no-overwrite
fragment publication. Episode capture freezes/restores its composition context
without rendering media, binding a runtime or adopting a source owner. The
private fragment supplies the source archive, selection, request and updated
metadata witness needed by the bounded episode job.

Both episode CLI fixtures now obtain their initial context from the executable,
then hide the source checkout and old renders before execution. They assert an
episode summary, complete readback, no rendering/job adoption during capture and
refusal to overwrite an existing fragment. The first capture-enabled mixed
episode run passes in 279.08 seconds. The strict request-family unit test and
existing scene-capture regressions protect the shared command's compatibility.

Final local qualification passes strict locked all-target/all-feature Clippy,
229 enabled library tests, 88 enabled asset-resolution tests and both complete
CLI variants together in 256.84 seconds. Formatting and whitespace checks pass.
Hosted qualification remains required.

A real S2E10 capture against qualified Melena source also passes: one root,
24 composition controls and a 5,303-byte private fragment restore exactly in
the existing F: metadata store. The command reports no media rendering, source
owner adoption or eviction authority. This is real composition-metadata
acceptance, not proof of every component's primary closure or managed production.

Producer-applied provenance review requires exact committed-source and definition
pins, private fragments, retained witnesses and independent restore before use.
Editor/sound review leaves component eligibility and decoded output checks with
the contained worker; a captured composition alone grants neither. These are
checklist findings, not independent creative or distribution approval.

Actual project capture and managed execution remain distinct acceptance steps.
This command installs no production binding, discovers no cache, initializes no
store and authorizes no payload eviction. Full project cutover remains open.
