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

The first frozen native fixture at source
`3a972f8b419bb17f2097230d8118fe31a8d7ba4c` refused its preflight in 105.95
seconds with `Stale/native-task-commit`: its cloned fixed-only profile had gained
upstream origins without changing to the required composite schema. The refusal
preceded any close ticket, operation marker, admission, journal or native output.
Raw source/tools/logs and the owned candidate source remain preserved under
`F:/src/tig-dev-temp/reel-native-repair-control-20261120-v1`. The fixture now
declares its bounded composite-v1 schema explicitly and saves preflight controls
before checking them. A separate read-only audit must reopen the original part's
exact activation and unchanged actual Index before any explicit replan.

That audit passed in 0.32 seconds at source
`6f78179b2595f8e1f04f7459171c267463ef8af5`. The corrected fixture under
`F:/src/tig-dev-temp/reel-native-repair-control-20261120-v2` passed source/byte
preflight, closed the original part with its original 264efd runtime, and admitted
65 declared resources (189,660,512 logical bytes) in slot2. The worker produced
the new control, but managed import stopped after 678.31 seconds with
`Budget/handoff-retain:retention-apply:retention-resource-bound`.

The original operation is `rooted-issue-control-original`, attempt
`rooted-control-attempt-1`, task
`c116b7dddd59a15c8d28eee16831bf9ac8594f99fc4a19d5c7e39c7f4bc47589`.
Activation is `25e841600e8aa93861877f357eae98b84443b29769501862e646790bb5f119e6`,
source WorkRef is
`work-63343abc3b3600ed91e97a312fa2d5b25076516372e74000b735f902c9370d25`,
and source commit is `c184b3db9b94e6001b91c3b69c2546a82750b7c1` in the owned
`F:/src/tig-dev-temp/.tmpr8GNVM/source-history` repository. The journal remains
ImportPending with no retained acknowledgement/task completion established.
Source protection, published/uncertain objects, owned scratch, exact runtime and
all original handles must be preserved. Do not restart the fixture or retry effects
under another operation, source snapshot or runtime.

The existing registration code bounds the union of the source and cumulative
retained-owner history using the job's projection limits (v1: 512 objects / 1 GiB).
This check is distinct from the 128 changed-path phase limit and the 65-resource
initial closure. A new read-only `inspect-retention-capacity` command reports exact
source/history union counts, bytes, delta paths and original retention-work status,
while reopening the original activation and validating completed response metadata.
It neither reads native output payloads nor changes budgets, creates retention
work, returns a job slot or acknowledges completion. Its opt-in pending-operation
diagnostic is now [qualified](2026-11-20-retained-owner-capacity.md): 525 combined
objects exceed 512; 270,429,684 logical bytes and 52 new paths remain within their
respective 1 GiB / 128 limits. No original retention work is staged. Actual Index
state and original controls remain unchanged. Bounded original-operation capacity
recovery and complete corrected-issue retention still remain open.

Producer-applied comics-editor and rights/provenance checklists preserve source
wording, stable targets, exact lineage and private technical derivatives. They do
not establish viewing, independent review, creative/font/translation approval or
release. General mixed-snapshot reviewed-producer binding and all remaining
S5/BBJ/project scale and adoption requirements remain open.
