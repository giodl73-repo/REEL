# Explicit public SDK client setup

This is an experimental Rust route, with the exact public SDK revision in
Cargo.toml/Cargo.lock. Use the existing CLI for inspection and queries; consume
this package and the SDK for managed handoff protection/publication. The CLI does
not imply a managed Lifecycle. REEL owns task semantics and provider/manual work.

The executable synthetic reference is [lifecycle.rs](tests/native/lifecycle.rs)
and its [client helper](tests/native/lifecycle/client.rs). It uses ordinary/bare
fresh fixture pools. Actual caches, actor/epoch admission and project acceptance
are separate from this recipe; nothing initializes or migrates them on resume.

## Original setup, before provider or manual dispatch

1. Admit the actual pool profile/full namespace roster and trusted original
   metadata/witness/capacity handles. Explicitly enroll sources with the existing
   `Namespace::apply(Command::Enroll)` at owned original operation IDs: one Job
   for inputs; three distinct Retention sources for controls, returns and the
   final retained cohort; separate Retention ingress sources for individual
   control blobs; and separate Retention sources for candidate publications.
   Enrollment is an effect, never a side effect of `Lifecycle::reopen`.
2. Capture the exact immutable Git commit/scopes with `Snapshot::capture`, then
   `preview` the explicitly declared input closure. Preserve the original packet,
   request, preview and independently known tool bytes/artifacts. Include native
   and upstream declarations when used. Small authored definitions remain in Git;
   runtime controls and evidence stay asset-side.
3. Build `binding::resources` and the exact Job tree, prepare its original
   `claims::Handle`, and prepare three `retention::Seed`s from the enrolled empty
   sources. `Binding::capture` binds these seeds, the original handle, logical
   binding ID and exact packet/control pins. Build `Binding::control_selection`
   only after the immutable binding bytes exist: no self-hash cycle.
4. For each original control (including the binding), construct
   `ingress::Control::new(artifact, raw_bytes)`. Allocate the original SDK
   `publication::Request` with its distinct ingress source/operation, exact
   candidate and explicit provenance; call `control.plan` with the binding,
   admitted publication profile, original SDK Admission and metadata NodeStore.
   It observes the actual configured namespace, requires an empty Retention
   source, refuses the Job/three composite seeds, and only freezes metadata.
5. Persist `control.encode_plan(&plan)` and its independent expected `Object`
   BEFORE native publication. These are the SDK's own Plan bytes, not another
   journal or plan schema. Persist the metadata checkpoint and original
   witness/capacity/namespace references too. Own this bounded recovery control
   outside disposable scratch, or through an already retained caller control.
   Hashing data is not authentication or retention. A lost/untrusted original
   plan cannot be repaired by generating new operation IDs.
6. Finish/reopen metadata, then explicitly call the existing
   `Publisher::apply(&plan, resources, &mut control)` for each blob. Keep all actual
   SDK Ready guards alive through `control_seed.admit` of the composite control
   selection. Each ingress source retains its own blob before that admission;
   never consume the composite seed's first revision with a blob publication.
7. Admit the exact input job through `GuardedInputs::admit_claim`. Create
   `handoff::Lifecycle` in Active mode and call the existing REEL
   `Journal::create`: Waiting is recorded only after the lifecycle reopens the
   original binding, actual controls, original input generation and native bytes.
   Live guard release/process exit does not withdraw durable claims. No TTL or
   timeout deletes referenced bytes; cancellation remains an explicit task/source
   operation and must not cancel an uncertain publication.

## Original returned-result ingestion

1. Bind the original returned JSON bytes with `Candidates::bind`, preserving
   whitespace and provenance. Freeze every original output/receipt/return-packet
   SDK Plan and call `Candidates::prepare`. Preserve its original plan control.
2. Publish raw return and frozen-plan control bytes using two separate ingress
   sources through the same `ingress::Control`/SDK sequence above. Keep actual
   Ready guards through result-seed admission of exactly `packet` and `plans`,
   context bound to the original Binding pin.
3. Build `handoff::final_selection` and finish/reopen metadata BEFORE output
   effects. `Journal::import` records ImportPending before Lifecycle invokes
   the original frozen candidate publications. It admits independent final-cohort
   retention and reopens actual bytes before the REEL Retained event. All guards
   survive that final journal append. This is no creative selection/approval or
   delivery, and grants no scratch-retirement/deletion action.

## Reconciliation after restart

`Control::restore_plan(artifact, original_wire, independent_expected_object)`
restores the exact bounded SDK Plan without original bytes or storage IO. Use the
original profile/metadata/witness/ledger references to call SDK `status`,
`resume` or `reopen`. If verified original input is still needed before any effect,
reconstruct Control from those exact pinned bytes and reapply the SAME plan.
After a possible native effect, SDK uncertainty never authorizes another upload.
Do not call `Control::plan` again against an advanced source: it deliberately
refuses, and cannot replace the original recovery handle.

Resume the original REEL Journal, not a new attempt. Active work reopens the exact
snapshot/inputs and durable original job. RetainedOnly verifies original controls,
job-admission receipt, original Ready candidates and final cohort without Git,
input bytes, staging, producer input or a revived Job. A completed native effect
with lost workflow acknowledgement can finish the original ImportPending entry;
an uncertain native effect stays pending. Neither file presence nor a decoded
receipt is a substitute for that retained reopen.

## Bounds and errors

Control bytes: at most 1 MiB per immutable owned value. Encoded SDK Plan: at most
64 KiB; verify length/hash before parse and accept only the original encoder's
bytes/guarded one-shot protocol. Callers bound aggregate counts/memory and SDK
metadata/capacity/read/time budgets; this API is per object, not a batch quota.
Binding/control retention and media budgets remain those in README.md.

| Boundary | Meaning / allowed next action |
| --- | --- |
| Integrity / Invalid / Budget during Control or plan decoding | No publication; fix trusted input/budget, preserving the original identity. |
| Missing / Denied / Stale during plan preparation | No publication or claim admission; correct the explicit enrollment/authority/baseline. Completed sources require original-plan recovery. Metadata freeze failures may leave unselected immutable nodes. |
| SDK Busy or admission failure | Preserve original plan and state; retry only through the qualified SDK's original-operation rules. |
| SDK uncertain / NeedsRecovery after a possible effect | Retain all original handles and bytes; status/reconcile original operation, never fresh publication. |
| Ready or REEL Retained | Actual guard-backed observation at this point, separate from human acceptance and creative selection. |

Synthetic setup/recovery qualification does not close mixed legacy pruning,
production enrollment, full workload lifetime, clean-machine packaging or official
project cutover. Follow PRODUCT_PLAN.md P2/P5/P6 for those separate gates.
