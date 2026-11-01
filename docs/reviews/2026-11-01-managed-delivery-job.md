# Managed delivery jobs

Entry `0ab3223f8dc0ca4c10d3db6c7be9ec24e08348cc`. The existing JobPlan now
holds an explicit private scene-or-delivery request. Its delivery constructor
derives the already checked recipe and reuses the same runtime map and job
freeze/readback/protect/publish/adopt machinery. No parallel delivery journal,
fence, cache, resource ledger or retention implementation is introduced.

Scene-only request access is fallible. Current-scene matching returns false for
a delivery, and scene reference/control compilation refuses before media or
preparation writes. The compiler found three additional direct scene-field
accesses in the reference checker; these now use the same checked accessor.
Legacy public scene constructors/render signatures remain unchanged.

Producer storage/concurrency/editor/sound/privacy review: exact original journal,
owner, revision, context and candidate stamps remain required. The delivery
producer still binds runtime files, exact primary bytes and memory controls;
the shared renderer still checks the candidate recipe against its job plan.
The proposed preparation directory remains unused, never an implicit copy root.
Resource capacity and process containment remain the enclosing runner's work.

The synthetic native fixture reuses the unchanged native-job lifecycle suite
for the new delivery plan: missing runtime metadata, stale/foreign ownership,
publication, output readback/adoption, and post-handoff refusals. It also rejects
scene-reference compilation of the delivery plan and leaves that directory
absent. These are scoped SDK tests, not independent approval, global custody or
actual-project cutover. Worker/CLI and project adoption remain open.

Final local checks: ten regular template/context cases, the delivery managed-job
lifecycle/publication fixture, the existing complete scene/CLI lifecycle, and
all-target/all-feature warnings-denied Clippy pass. The legacy CLI fixture needs
its explicit proposal executable; a missing binding now refuses before rendering.
The corrected run used a hash/length-pinned executable built from the checked
vendored Tig source. No runtime was installed or project binding changed.
