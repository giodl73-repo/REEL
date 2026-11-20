# Explicit part repair with historical consumers

The retained two-page issue exposes a shared-input case: both `opening` and
`closing` consume `comic/master` and `comic/plan`. A page-one lettering revision
changes those controls, although the existing page-two part remains valid under
its original exact snapshot. Global repair v1 conservatively rebuilds both parts.

`reel.workflow-repair.v2` adds mandatory, explicit `roots`. Each root must directly
reference an exact edited artifact; duplicate, missing or empty roots refuse.
Every edit must occur within the selected roots' transitive dependency closure.
All consumers in that closure rebuild, including derived controls, full issue
assembly and reports. Unselected branches keep their exact old references.
This is an authored version choice, not automatic inference that a shared edit
has no effect elsewhere. Include every branch intended to receive shared art or
language changes. Omitted branches remain historical editions/parts.

Rooted repairs require existing repair-revision v2, which produces plan v3 with
complete per-task snapshots. Exactly the rebuilt native tasks require replacement
declarations. Other task definitions, snapshots and packets remain unchanged.
No result is carried into a changed packet. Embedded control JSON and future
parent outputs are never guessed or rewritten: authors must supply changed
controls and freeze actual parent results before native execution.

Legacy repair v1 omits `roots` and retains its serialized bytes and global impact
behavior. A v1 envelope with roots or a v2 envelope without roots refuses. Rooted
impact uses `reel.workflow-repair-impact.v2`; it pins the entire repair, including
roots. The checked baseline report and complete original review copy stay bound.
Local rebuild scope does not waive full-issue review or affected-language checks.
Resolution still needs a newly pinned complete issue and explicit contextual
evaluation; targeting one finding does not close another finding.

Three new workflow tests cover shared-input branches, Spanish/English isolation,
unchanged actual packets, legacy serialization/global behavior, version/root/edit
refusals, revision requirements and complete contextual-resolution requirements.
The full workflow suite passes 137 tests and strict all-target Clippy.

The opt-in native fixture `repair_roots_native` reopens the existing native report
and original whole-issue proof, checks local deterministic replay against the
retained issue receipt/output pins, then makes a local lettering-only candidate.
It checks unchanged clean art, every closing-part byte, Spanish wording and stable
IDs; changed reading pixels; and replay of both pages. It exports a rooted plan
and two explicit private issue drafts through the actual CLI, refuses a stale
report, compares the real Index checkpoint and reopens the other Waiting job.
It makes no provider/tracker request, new admission, selection or closure.

The fixture's new Git commit contains authored candidate sources. Its pending
native declaration files deliberately identify themselves as unqualified; they
are planning placeholders, not executable/admitted source profiles. Descendant
control inputs still need regeneration from fresh retained parents. Local replay
does not establish corrected-issue native retention, task completion, actual
viewing or production readiness. Those integration gates remain open, as do real
BBJ scale, film/S5 scale, owned adoption/rollback and the rest of the ledger.

Producer-applied comics-editor, editor and rights/provenance checklists preserve
full-issue context, exact historical lineage, private draft boundaries and separate
approval. Synthetic findings and a technical font-size change assert no actual
creative defect, translation approval, independent review or release authority.
