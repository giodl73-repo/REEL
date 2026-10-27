# Bounded native worker CLI

Entry06b6651, qualified197/mainf82d455, upstreamfc2a9dd. WP-004/006/008/009/010/011/012,
REQ-001/002/003/004/006/007/008/009/010, SPEC-003/004/005/007/008/010/011/012,
IF-002/004/005/008/010/011/012, PKG-003/004/006/008. Scope: optional Rust native
job coordinator/worker CLI over existing frozen context, runtime binding, owner
journal and admitted bounded runner. No setup, daemon, cache/registry discovery,
live media, transfer/removal or consumer cutover.

Use the same actual pinned CLI executable for coordinator and contained worker.
The coordinator binds its host/FFmpeg/FFprobe/dependencies, restores an explicitly
admitted frozen native context, freezes/readbacks and owner-adopts JobPlan, then
runs its worker through the existing Tig-backed budgeted runner. Retain runtime
guards across the child. Worker rebinds its own actual executable and explicit
programs, restores the same context/recipe, renders and independently checks.
Do not invoke the legacy ambient-PATH renderer as a hidden fallback.

The worker does not acquire the coordinator's job fence or rewrite its journal.
It receives a bounded private immutable admission through an inline job file;
inputs stay in existing caches and native InputFiles resolves exact primary pins.
Declared output sizes and scratch/time budgets are explicit. All native receipt
outputs plus the receipt must be declared before the request runs; an incomplete
set refuses and cannot report successful delivery. Generic runner retains the
verified outputs and failure evidence, adopts their root, then retires scratch.
No opaque serialized success JSON creates a RenderedCandidate or production claim.

Job-request validation must finish before metadata/owner transitions. Existing
journal/metadata/cache/control roots and minimum checkpoints are mandatory; no
initialization or automatic reset. Duplicate/unknown/oversized profiles, stale or
foreign ownership, wrong runtime bytes and mismatched context refuse. Errors keep
durable requirements and preserve ambiguous retention workspaces. Complete global
claims/accounting, native claim export, crash recovery CLI, full-family migration,
archive custody/eviction and production adoption remain open.

L0 fmt, locked native/donor tests, warnings-denied Clippy/feature isolation.
L1 strict bounded intake, invalid budget/output/name/runtime/context/owner checks
before execution, unchanged legacy CLI, worker refuses incomplete output sets.
L2 actual CLI coordinator -> contained same-binary worker -> native FFmpeg render
and independent checks -> retained outputs/receipt -> owned root handoff -> exact
scratch retirement, with Git/working metadata offline and catalog absent.
Rights/Provenance, Editor, identity/concurrency/provider/custody producer lenses;
no simulated human approval. Parent197 must qualify/merge before final consumer
qualification and squash merge of this package.

## Executed design and review

The coordinator and worker use the same actual executable, not a caller label
for a different host. Verified runtime guards remain held in the coordinator
through child completion; the child independently rebinds the declared host and
programs. Its ticket is bounded and pinned in the coordinator's process arguments;
changed bytes refuse before profile parsing, runtime or metadata lookup. The child
checks the frozen context, exact producer and recipe, runs native render and
independent verification, and refuses a different or oversized output set.

The coordinator validates the complete generic runner request before metadata
or journal transitions, then uses existing JobPlan/provider readback/protection
and the existing Tig runner. It does not acquire a second job fence in the child
or maintain a second list of asset dependencies. The worker's source profile
supplies explicit existing-byte locations; primary files execute/read in place.
Small temporary request controls are private, bounded and outside Git. Scratch
and atomic publication allowances remain explicit conservative budgets.

The actual Windows fixture passes coordinator -> contained child -> native
render/check -> owned retention -> scratch retirement with Git, records and
definitions physically renamed offline. It checks every retained output's hash
and size, receipt sample/frame counts and output names, completion/receipt pin
agreement, owner revision4 with an input predecessor, empty scratch and no hidden
catalog/registry. An invalid budget leaves both metadata checkpoint and journal
unchanged and never creates staging. Full donor and normal consumer regressions
pass. Final ticket-pin native rerun passes, including an extra-output-set mismatch:
the child refuses success, the coordinator retains bounded candidates/diagnostics
under a FAILED result root with its input predecessor, and scratch retires. The
earlier successful source remains unchanged. Final actual case passes178.81s;
focused ticket-refusal unit, fmt, warnings-denied donor/root all-target/all-feature
Clippy and default/read-only Tig checks pass. Hosted final-head qualification is
still required before this package merges. Initial mismatch-case assertion was
corrected to accept the existing quoted diagnostic URI; no production behavior
was changed to satisfy that test.

Rights/Provenance producer lens: pass_with_risk for private technical execution.
Profiles/receipts contain private local configuration, not public redacted
diagnostics. No upload, creative/consent/delivery status or reusable producer claim
is inferred from a child JSON record. Generic retention stays unproven and
complete claim coverage stays false.

Editor plus identity/concurrency/provider/custody producer lenses: pass_with_risk
for the explicit coordinator/contained-worker integration. Runtime verification
is declared, not full environment discovery. Another scene/job keeps its own
source fence. No source/cache payload removal occurs. Remaining gaps include
complete global accounting, owner crash-recovery commands, claim export and
canonical project handoff, computed output-budget planning, all-family coverage,
archive custody and reversible adoption. These are producer checklist findings,
not independent reviewer or human acceptance.
