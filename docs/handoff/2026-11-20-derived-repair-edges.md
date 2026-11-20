# Rebinding repaired dependency results

The retained repaired part has a new worker-receipt identity and new editable
bytes. Its downstream control also produces a task-key-named assembly artifact.
Those are observed producer results, not same-asset version edits. The existing
same-logical-ID rule for `review::Edit` stays intact.

`reel.workflow-repair-revision.v4` extends v3's explicit task snapshots with a
mandatory `derivation`. Each entry names consumer before/after upstream bindings
and complete before/after packet/result witnesses for the required transitive
parent closure. Parent task IDs and output/worker-receipt selectors must stay the
same. Parent packets must exactly match their respective baseline/candidate task,
project, snapshot and dependency-result fingerprints; returned-result checks still
apply. Missing, extra, foreign, stale or contradictory ancestry refuses.

Materialization updates only declared input artifacts, source-path bindings and
input-backed producer references (publication, request or part plan), plus the
exact upstream-binding pointer. Replacements are simultaneous, including when
logical IDs exchange positions. Runtime/code and native-binding controls cannot
be replaced through a data edge. Asset edits overlapping those upstream inputs
or their binding control refuse. Fixed-input identity collisions still refuse.
Opaque control JSON, prompts, dependencies and output contracts are not rewritten.

The initial root can remain at source B while controls use C. Finalizing a later
consumer from a newly observed control result leaves the root and already frozen
control packets unchanged. The generated plan remains v3; worker packets and
native admission schemas do not change. V1/v2/v3 revisions omit `derivation` and
keep their original JSON behavior; a derivation in those versions refuses.

Witnesses are bounded metadata, not proof of native custody or actual execution.
The complete revision stays within the existing 1 MiB control envelope and
128-task/input limits. New native consumers must still obtain their own admitted
closure using actual retained parent controls. Neither a matching payload hash
nor a successful private plan export substitutes for that lifecycle boundary.
No candidate completion, selection, finding closure or approval follows.

Four new workflow tests cover staged finalization, exact root/control packet
preservation, changed request identities and receipt/source paths, source/parent
fingerprints, wrong selectors, stale bindings, overlapping edits, legacy JSON,
simultaneous identity swaps, fixed-ID collisions and runtime replacement refusal.
All 143 workflow tests and strict workflow/native Clippy pass. These are metadata
checks, not native execution qualification.

The opt-in `repair_control_native` fixture uses the actual retained corrected
opening and exact historical closing. It authors an isolated source C containing
the needed fixed records, supplies real before/after witnesses, checks a
deterministic preflight, then guards the original part's close and same-slot
handoff. Actual parent proof and child admission precede owned production and
retention of the new assembly control. It must verify both parent histories,
the other Waiting generation and completed replay with unchanged actual Index.
This fixture still requires frozen-source qualification. It cannot stand in for
retention of the full corrected issue, contextual review or native later repairs.

Producer-applied comics-editor and rights/provenance checklists preserve source
wording, stable targets, exact lineage and private technical derivatives. They do
not establish viewing, independent review, creative/font/translation approval or
release. General mixed-snapshot reviewed-producer binding and all remaining
S5/BBJ/project scale and adoption requirements remain open.
