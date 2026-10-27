# Native original-claim replay contract

Entry REEL 5e49521f14aeda72a8dcb42bf56f710e365c618c (PR191, stacked on
PR190; both qualification pending). Continues WP-010/011/012, REQ-006/009/010,
SPEC-010/011/012, IF-005/010/011, PKG-004/006. Optional metadata-only Rust API,
shared native policy derivation and synthetic regressions; no CLI/runtime switch,
provider initialization, real media render, upload, selection or cache mutation.

Expose native restore_claim using external original ArchiveProfile, request,
producer Object, exact parsed Claim and caller-owned metadata provider/bounds.
Reject a different operation/request/producer before store reads. Share recipe
operation and consumption derivation with planning; never introduce a separately
authored production asset list. Use qualified upstream original-claim replay,
including full context/source/parameter/policy checks and output/input size
consistency. Then require the externally admitted archive/checkpoint and native
scene root/descriptor, returning the verified Source and upstream ReplayedClaim.
Bind the restored source to that exact native request as with context restore.

The claim is unsigned metadata: replay proves original context and recipe
correspondence, not producer execution, actual output bytes, approval, current
delivery, custody or retirement. Present historical outputs without claims remain
selected/retained history, not fabricated native proof. Matching recipes do not
bypass native compilation eligibility or full runtime/tool admission.

L0: exact vendor bytes, fmt, default-library isolation, all-feature/all-target
warnings-denied Clippy and diff. L1: operation/request/producer mismatch before
store IO, source/archive/checkpoint/admission/selector/header/recipe/size mismatch,
missing/corrupt metadata, zero writes and zero payload IO. L2: real temporary Git,
finished/reopened metadata and source physically unavailable; replay original
recipe after main changes, preserving semantic match for annotations and stale
status for changed consumed inputs. Extend the tiny actual producer test through
claim parse and source-independent native replay; no user media or cache changes.

Rights/Provenance and Editor lenses review source/recipe/execution boundaries and
independent historical jobs. These are producer checklists, not independent or
human acceptance. Full runtime closure, durable publication/custody, complete job
and capacity enrollment, variants/families and reviewed adoption remain open.

## Local qualification and producer review

Windows: all 27 normal Tig consumer tests pass. A real temporary Git main advances
through annotation-only and primary replacement commits; after SQLite finish/
reopen and physically moving Git/records/definitions aside, original native claim
replay preserves exact provenance and reports claimed-match versus stale against
those independent desired plans. The returned source rejects a changed request.
An instrumented metadata port asserts zero writes; successful replay/recipe use
zero payload reads. Wrong producer/language refuses before any metadata read.
Wrong source checkpoint/archive, definition/selector admission, missing metadata,
forged recipe/binding/header/root, conflicting output/input sizes and tiny budgets
also refuse through this native boundary.

The explicit real tiny Windows producer test also passes: its actual checked
candidate claim is parsed, the metadata reader closes, Git/working metadata move
offline, and a new reader plus native restore_claim returns the original context
and claimed recipe match without payload reads. This does not reconstruct a
RenderedCandidate from a receipt or grant fresh execution/custody authority.
All-target/all-feature warnings-denied Clippy, default-library isolation, fmt,
exact vendor integrity and diff pass locally. Exact-head cloud qualification and
qualified parent merges remain required.

Rights/Provenance finding: pure plan checks alone must not masquerade as original
context verification; the native API now delegates to full upstream replay before
returning its checked claim. Editor finding: preserve one native policy and
consumption derivation for planning and replay; shared helpers enforce that seam.
Producer disposition pass_with_risk for metadata correspondence only; integration
owns runtime/tool, output publication/custody and adoption gaps. Not independent
review or human acceptance.
