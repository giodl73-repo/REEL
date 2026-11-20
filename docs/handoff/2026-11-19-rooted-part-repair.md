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
The frozen fixture passed in 87.28 seconds at source
`67d2f38b9676caaa199a207528c609a121bd203e`. Its five-task rebuild is `opening`,
`issue-control`, `issue-assemble`, `owned-issue-review-controls` and
`whole-issue-structured-report`; `closing` retains its entire original packet.
Only the page-one finding is targeted by the candidate. Both scripted findings
have separate private drafts and neither is closed.

The original request and worker receipt are read from their exact native pins.
A first fixture at `ea34080622785943d12e3e799b94831d0c22078e` instead tried the
restored local `source-assembly.json`: that reformatted copy was 1,278 bytes and
failed the actual 994-byte request pin before candidate rendering/export. Its
exit-101 log and source/tools remain in
`F:/src/tig-dev-temp/reel-rooted-part-repair-20261119-v1`. The corrected fixture
does not accept a repinned substitute or change the original operation. Native
proof and exact controls precede local replay; the replayed receipt and both
outputs must match the actual retained issue.

All evidence is preserved under
`F:/src/tig-dev-temp/reel-rooted-part-repair-20261119-v2`:

- Source archive: 1,971 files verified against the clean tree; source-manifest
  SHA256 `6bff396ef08e874974bbe645f00b41fb121aadd5046134eced5b48320fd4c9da`.
- Frozen CLI: SHA256
  `264efd3655caa8f0190ab00548e6daff3a3b2a7a457261fd7d27c68d46e12b9a`,
  24,826,880 bytes. It performs read-only proof checks/private exports; it does not
  replace the report's original lifecycle runtime.
- Frozen fixture: SHA256
  `c71fe4e1df6fffe30d81cc444151beac5f1d77e0590b42b80ca6b7373bcd94ea`,
  29,616,128 bytes.
- `qualified-report.json`: SHA256
  `db0b900c09b0c795dbce1e41b257e5a8787257261faa1b84c1e4d3ea91125ae3`.
  It pins the controls, generated local packet, drafts, source, tools and run.
- `qualified-validation.json`: SHA256
  `e0c7879a28e6058199c74f1d65ea6b36fc8f42bf3ee0d629130d8c1fb25b04d7`.
  All 137 workflow tests and 86 standard native tests pass, with 78 opt-in native
  tests excluded from that standard run; the rooted fixture is qualified
  separately. All 247 SDK source pins and strict workflow/native Clippy checks
  with both native feature modes pass.

The actual native Index checkpoint remains
`4ab1a854540d50c0eba060a46c12727519a1425548ff13c10b1728261eb374b1`, equal to
the previous retained report qualification before and after this run. Original
control/packet/result pins remain unchanged and the unrelated Waiting generation
reopens. No active actor is closed, reused or retargeted by this qualification.

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
