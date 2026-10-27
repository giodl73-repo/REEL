# Admitted runner resource and retention adapters

Entry REEL6283e2600b359e19bf044f863313636d2ab665ec, pending196; upstreamfc2a9dd.
WP-004/006/008/009/010/011/012, REQ-001/002/003/004/006/007/008/009/010,
SPEC-003/004/005/007/008/010/011/012, IF-002/004/005/008/010/011/012,
PKG-003/004/006/008. Scope: existing reel-assets job runner/lifecycle, explicit
resource-authority and retention ports, optional Tig adapter, synthetic fixtures.
No second runner/provider/cache, daemon/setup, live media/removal or cutover.

Existing run/resume commands retain registry behavior. Add an explicitly admitted
run entry taking Config, a typed role-sensitive resource authority and a retention
sink. Named input/tool/installed-tool/checkpoint membership is checked before
capacity reservation or source-path lookup. Reuse the existing reservation,
read-only input views, bounded workspace monitor, atomic Windows process-tree
containment, output verification/publication and exact owned cleanup. Root project
catalog/selected() and registry registration must not be hidden dependencies of
this new route. Root/path config remains explicit; no archive discovery.

Retention receives immutable internally constructed result descriptors for exact
request/evidence/declared outputs, including success versus salvaged failure.
It must durably establish externally admitted owned references before returning
success. Registry compatibility sink keeps existing registration behavior; Tig
sink uses existing metadata finish/reopen/readback and SourceJournal prepare/adopt.
No native execution/approval claim is fabricated for arbitrary runner commands;
generic results remain unproven technical output/evidence. Retention failure keeps
workspace. Cleanup still independently verifies retained inputs/outputs and exact
workspace path; only owned disposable scratch may retire, never cache/originals.

Tig authority derives named resource membership from an admitted planned job
context, with runtime-only tools and primary/control/interpretation inputs. Retained
result roots link the original job selection and exact verified result pins,
without another editable inventory. Match journal identity/owner/revision/current
selection; reject foreign/stale source before cleanup. Complete coverage, native
managed CLI/producer claims, global capacity accounting, archive custody, all-family
migration and reversible adoption remain open.

L0 fmt, locked donor/consumer tests, warnings-denied Clippy/feature isolation.
L1 wrong role/size/authority before data admission, stale/foreign retention, failed
retention preserves workspace, missing retained data prevents cleanup and strict
metadata replay. L2 actual trusted child in temporary caches, enforced workspace
budget/timeout, Tig finish/reopen + owned result-root handoff and scratch retirement
with originals/output kept. Producer role lenses only; upstream and parent196
qualification/merge precede final-head qualification and consumer merge.

## Implementation and review findings

The admitted route supplies Config, ResourceAuthority and RetentionSink to the
same run_loaded/lifecycle path. Compatibility entry points construct registry
ports; checkpoint resume remains on that route. Single bounded request intake
also removes the former separate raw-read/parsed-read race. Exact role membership
precedes reservation, installed-tool opening and CAS lookup. Immutable retained
descriptors are built only after the runner publishes verified request, evidence
and bounded declared outputs.

Successful-producer retirement failures now return immediately with staging
preserved. Review found that falling through the old execution-failure handler
could call a retainer a second time with contradictory failure evidence after a
successful producer. The new fixture injects that failure, checks one callback
and preserves both result.json and original output. Existing diagnostics retain
their staging-preserved wording; the full donor regression caught and corrected
an initial wording change.

The optional Tig adapter shares native job coordination derived from actual
journal identity/source, checks owner/revision/selection, and adopts a linked
result tree only after metadata finish/reopen/exact readback. It retains the
original input predecessor. Request/evidence and each output remain exact byte
pins; arbitrary commands never gain a native production claim. Metadata and
journal outcomes that are uncertain keep workspace/requirements for recovery.

Rights/Provenance producer lens: pass_with_risk for private technical retention.
The compressed job evidence may contain private paths/control/logs and is an
asset-side private artifact; it is not a shareable redacted receipt. There is no
upload, approval, consent, source eviction or delivery authority. Runtime-role
membership does not discover the installed environment or establish licensing.

Editor and Tig identity/concurrency/provider/custody producer lenses:
pass_with_risk for reusing one bounded runner and avoiding a second editable
inventory. Same-job fencing must not become a whole-cache/render lock; the
existing concurrent donor cases remain required. Only exact disposable scratch
is retired after owned retention and fresh byte checks. Positive linked roots
are not complete global claims or independent retained custody. No independent
reviewer or human acceptance is asserted by these checklist findings.

Open boundaries: the generic result envelope is not yet a complete native/global
claim interpreter or recovery CLI; native render CLI still needs bounded-runner
dispatch integration. Full physical-capacity accounting, metadata availability,
all-family rehearsal, custody/eviction and reversible production adoption stay
open. Existing retain_workspace diagnostics explicitly skip automatic handoff.

Executed fixtures: full reel-assets all-target suite passes (224 library cases
plus binary/integration targets); 27 normal native-consumer and eight existing
publication cases pass. The actual Windows native fixture passes role rejection
before staging, foreign owner, stale revision, same-job contention without journal
mutation, verified successful retention, enforced scratch-budget termination,
failed-job evidence retention, exact output-root ownership and owned scratch
retirement. The project deliberately has no catalog/registry. Its original input
root remains a predecessor and complete claim coverage stays false. All caches,
metadata and journals in these fixtures are synthetic temporary directories.
Locked warnings-denied donor and root all-target/all-feature Clippy, fmt, default
and read-only Tig feature-isolation checks pass. Final-head hosted qualification
is still required before this package merges.
