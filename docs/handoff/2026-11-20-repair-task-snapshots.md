# Later consumer commits without retargeting a repaired root

A protected part repair commits its exact seven-input source before rendering.
Fresh issue controls authored from its retained result may need another source
commit. Repair-revision v2 assigns one commit to every rebuilt task, so using that
later commit would change the completed part's packet even if all its other
descriptors were preserved. Its existing result cannot satisfy that new packet.

`reel.workflow-repair-revision.v3` adds `task_snapshots`, naming the exact commit
for every task in the repair impact's rebuild closure, including non-native
tasks. Missing, extra, zero, malformed and unknown task snapshots refuse. The
overall `source_commit` remains the authored revision identity; it never supplies
a missing task snapshot. Native declaration edits still cover exactly the rebuilt
native tasks and need their normal independent source/custody checks.

This allows a part at commit B to keep its exact B-bound declaration and packet
while fresh controls/assembly use commit C. Unaffected tasks keep their previous
task definitions and snapshots. The result is the existing plan v3, with a complete
task-to-commit map; worker packets and native protocols have no new schema. A
candidate grants no authority to carry results into a different packet or to
retarget an existing admitted job/runtime. Future parent outputs and changed
control bytes must still be supplied explicitly and bound to actual results.

Revision v1/v2 omit the empty map, preserving their original JSON and hash
behavior, and refuse a nonempty map. V2 retains its one-commit rebuild behavior.
Versioned repair roots and complete contextual review requirements are unchanged.

Two new tests prove exact root packet preservation across B/C consumer revisions,
unchanged independent/language packets, declaration scope and contextual impact,
legacy behavior/serialization and strict map/version refusal. All 139 workflow
tests and strict workflow Clippy pass. The opt-in
`retained_part_packet_survives_native_v3_revision_export` follow-up checks a real
retained part before read-only native CLI export, exact plan/root-packet equality
and an unchanged actual Index checkpoint. It still requires frozen qualification.
This is metadata planning qualification; corrected complete-issue execution still
needs actual qualified results. It does not relax the existing ambiguous mixed-snapshot
baseline refusal. Repeated repair loops over such a completed multi-commit plan
still need an explicit authenticated reviewed-producer binding.

The concurrently running corrected-part job remains bound to its original frozen
runtime and source packet. This planning change does not adopt that job into a
new binary, modify its source commit, release protection or select anything.
