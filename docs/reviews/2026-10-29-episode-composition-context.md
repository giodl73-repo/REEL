# Frozen native episode composition context

Entry cc1e6a43ac218c39d7fbdaead617e6f0a5401c4b; PR233 remains under hosted
qualification. This branch must not change that PR's tested head. The next managed
episode step needs an actual assembly context and producer recipe, rather than a
single scene job with an episode label. Reuse Tig's existing metadata archive,
ProductionContext and recipe ports. No new provider, registry, cache or runtime.

Allowed scope: episode adapter, generated metadata fixtures and this review.
Qualified dependency amendment: Tig PR48 is green on exact 9660617 on Windows
and Linux; normal merge 9252a8a1822e99155e5d2bc9bdd4a96476cfe22f has the same
whole tree f31a075f53b5fdc00f3c47cbfeff65db84f05f2e. Import only the existing
Cargo/lock/license/Rust-source allowlist and update raw-byte pins, vendor notes
and exact-pin consumer assertion. No downstream source fork or runtime install.
The composition context retains the assembly/episode/scene source documents and
admitted definitions. It does not flatten rendered component masters into primary
inputs. Ordered component IDs, kinds and exact selected byte subjects are consumed
operation parameters. Mutable status and unrelated commits are not consumed.

This is the assembly-owned part of the full job. Child scene input contexts,
independent original claims, presentation currency, composition byte protection,
source ownership, execution and publication still need their gates. Freezing or
matching this context cannot authorize execution, delivery, approval or eviction.

L0: fmt, locked all-target/all-feature Clippy and default-feature isolation.
L1: leaf edits without parent edits, annotation-only edits, exact request/source/
producer binding, bounded descriptors and pre-write refusal, absent output bytes.
L2: durable metadata finish/reopen, original source unavailable, recipe replay and
old snapshot restoration; then reuse this context in the complete managed fake
episode. Hosted Windows/Linux/FFmpeg and downstream source pins remain required.

Producer checklist lenses: Editor requires explicit conform order and selected
component subjects. Sound Designer requires the exact language and episode clock
contract. Rights/Provenance separates metadata assertions from bytes, execution,
creative selection and release. These are producer checks, not independent or
human approvals.

```text
assembly -> episode + conform -> ordered scene source documents
   |            (retained controls, no child-primary reclassification)
   +-> ordered component IDs + selected bytes + definition pins -> recipe
   +-> component bytes / input evidence -> explicit Tig resource roles
child scene contexts + original claims -> separate freshness/protection gates
```

Producer review disposition: pass_with_risk for assembly-context capture only.
Component selections and definition changes invalidate the recipe without parent
edits. Status/annotation edits preserve it; producer identity remains consumed.
Production roots/interpretation must match the frozen context as well as its
descriptor. The child-primary closure is deliberately absent from this context:
a complete managed executor must combine independently protected child jobs and
current scene/presentation proofs before execution. Neither an empty primary set
nor a matching composition recipe can stand in for those gates.

Local qualification after importing Tig9252: all thirteen episode tests pass
(43.49s); both real-FFmpeg generated episode/cache lifecycle fixtures pass
(206.38s); exact raw vendor source pins pass (65 files, 694,972 bytes); locked
all-target/all-feature Clippy with warnings denied passes (81s). Default-feature
isolation passed before the optional Tig-only import: 192 library tests pass,
nine explicit external cases ignored. The first fixture compile caught missing
metadata-store limits; corrected with an explicitly bounded new temporary store.
No production invariant or assertion was relaxed. Broader normal native consumer
tests also pass: 71 passed, eleven explicit external cases ignored (199.61s).
The two generated lifecycle fixtures were run explicitly, not left ignored.
Formatting and diff checks pass. New-head hosted qualification remains required.

PR233 is now qualified on all three checks and squash merged as
feb2a2067a7b7cb21ada58c96855068147952a18. Whole tree
67bcc9973fe4bb312e03f20799bb9f25349a16aa equals exact tested cc1e6a4.
That parent gate does not qualify the additional source in this branch.
