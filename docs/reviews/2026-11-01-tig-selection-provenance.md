# Native consumption admission versus imported selection labels

Entry `f37f4e43c225756e3a7b1551ae336e6df6c79c12`. Scope: an additional
regression fixture and clarification of the existing delivery-template contract;
no implementation or runtime change. Related Tig controls: WP-010/011,
REQ-003/010, native source and production-context contracts. REEL owns consumption
eligibility; Tig preserves immutable status without interpreting project values.

Actual migration inspection found picture references carrying both an older
selected-production label and a candidate-review state. Native projections
preserve the latter as provenance and do not enroll production eligibility.
This is a migration decision to reconcile, not a reason to weaken the adapter.
The private controls and their expanded inventory remain outside this repository.

The synthetic regression exercises fifteen combinations: root, job control,
contract control, production control and consumed picture, each with missing,
candidate-review or draft native eligibility. Each also carries competing
selected-production and release-looking historical labels. Every combination
must refuse before any byte-provider request. Selected payload identity alone
does not override eligibility. Existing successful-selection and unused-draft
fixtures remain the positive controls.

Rights/provenance producer checklist: historical selection, current production
eligibility, human approval and public release remain distinct. Git/adoption
check: no imported asset, selected delivery, producer code, recipe identity or
installed runtime is changed. This is producer review, not independent approval.
Local validation: the fifteen-combination regression passes; the full regular
delivery-template suite passes eleven cases (three native FFmpeg cases remain
explicitly ignored in this invocation). All-target/all-feature locked Clippy
with warnings denied, formatting and whitespace checks pass. No media producer
code changed, so this slice does not repeat the separately recorded native
render/worker qualification at the entry revision. Hosted qualification and
actual-project authority reconciliation remain separate.
