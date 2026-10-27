# Checked candidate byte publication contract

Entry REEL4df2cadab35b4a014a3a9c65739a6330290c73e0, based on pending PR194,
with upstream Tig96177878c7db238958c20a5faf1eec9bcfd6f438. Continues
WP-004/005/008/010/011/012, REQ-001/003/004/006/007/008/009/010,
SPEC-003/004/005/007/009/010/011/012, IF-004/005/009/010/011/012,
PKG-002/003/004/006/008. Changes are scoped to the optional native candidate
handoff, a generic PublicationBackend composition, synthetic fixtures and docs.
No live cache/media operations, consumer switch, approval, cleanup or eviction.

A RenderedCandidate is issued only by native compile/render/independent check.
Its explicit admitted OutputBindings select exact generated payloads. Add an
explicit byte-publication method taking caller-owned operation/owner and required
acknowledgement, generic backend and bounded output mapping. Before backend writes,
verify mapping, native receipt and all selected output bytes; preflight every
backend object/level budget. Publish selected payloads first, native receipt next,
and the exact unsigned produced claim last. Return a non-deserializable result
only after every required acknowledgement. No unselected output, directory walk,
extra source staging, implicit target/drive selection or duplicated authored list.

Derive stable, domain-separated per-item operation keys from the caller operation
and item role/name, never its changing content. Reusing a caller operation with a
different payload must conflict, not silently allocate a new operation. Complete
retry can reuse verified cached objects without stream reads, but the candidate
must still revalidate its source files. Failed partial batches remain valid
immutable cache objects; retry must not roll back, delete or repair them. No
all-object atomicity, held whole-batch lease or retained custody is promised.

The result exposes the claim, native receipt pin and individual publication
acknowledgements for explicit later registration. Do not fabricate producer
claims for historical files or make a public constructor/Deserialize for the
checked candidate/result. No automatic Git/status/selection update. Input context,
producer/parameters and all job/custody claims remain caller-retained obligations;
a byte acknowledgement alone does not make that closure complete or authorize
source retirement. Multiple selected outputs may share bytes; their bindings
remain distinct, without copying bytes per subject.

L0: fmt, locked scoped tests, warnings-denied Clippy and feature isolation.
L1: invalid/duplicate/wrong mappings, changed receipt/output, capability budgets
before publication, partial backend failure and stable idempotent retry/conflict.
L2: actual tiny FFmpeg native candidate, existing CAS publication, fresh source
reopen/verification of output/receipt/claim and original-context replay. Native
fixture stays temporary and private. Rights/Provenance, Editor and Tig storage,
concurrency and provider producer checklists apply, not independent/human review.
Upstream28 and REEL194 qualification/merge precede consumer merge; rebase onto
current main and qualify the exact final head rather than reusing stacked checks.

## Executed verification and producer findings

Windows actual tiny FFmpeg fixture passes using the declared, byte-verified host
and tool manifest. The native source/context is finished/reopened before render.
The test publishes a checked master, receipt and exact claim through the existing
CAS provider; an injected receipt-stage failure leaves the valid master and no
claim. A new provider resumes exact operation keys, acknowledges all three objects,
and repeats without changing requests. Fresh verified reads recover all objects;
the reloaded claim is then replayed against its original retained context with
Git and working metadata moved offline.

Empty, duplicate and unknown output mappings, changed master/receipt, unsupported
acknowledgement and object capability denial refuse before any publication fence
or metadata write. A changed selected set under the same operation conflicts.
A new explicit operation publishes master + audio mix, with two distinct logical
subjects sharing the master: three claim subjects, two payload requests and four
acknowledgements including receipt/claim. Reordering mappings preserves the claim
and requests. Foreign owner refuses; original local candidate files remain valid.

The pure operation-key case passes: changing content preserves the operation key
so the backend detects conflict; role, logical output name and caller operation
remain distinct. All 27 normal native consumer cases and eight provider cases
pass. Workspace root all-target/all-feature warnings-denied Clippy and default /
read-only Tig library checks pass after fixing one needless test-closure borrow.
Formatting and whitespace checks pass. CI now enables tig-publication for this
explicit native render fixture, rather than merely compiling the gated helper.

Rights/Provenance: returned data remains unsigned technical correspondence, with
no approval, delivery or retention authority. Do not deserialize a success into a
completed native candidate; constructors remain private. Editor: mapping stays
explicit, only selected payloads publish, and shared bytes retain separate logical
bindings. Storage/concurrency: all capabilities preflight before writes; immutable
partial batches remain recoverable, without rollback deletion or a whole-batch
lock/lease. Provider/privacy: raw receipts, claims, operation records and bytes
remain private runtime assets; no extra source staging or inventory enters Git.

Disposition pass_with_risk, producer lenses only. A generic admitted backend is a
trusted contract, not cryptographic proof of arbitrary provider correctness. The
batch result is not retained custody or complete source/runtime/job enrollment;
it cannot authorize source retirement or current-output adoption. Native original
context and runtime manifest availability still require independent resource/claim
admission. Existing provider and upstream qualification plus exact final-head
Windows/Linux CI are required before merge. Historical unproven masters receive
no invented claims. Full CAMINOS readiness and intended-use gates remain open.
