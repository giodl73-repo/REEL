# Exact Tig payload selector producer review

Entry REEL0311a1e83dad8152708505ae7d2ab08ab21fd1cb. Bounded optional Tig consumer
parser correction, continuing upstream WP-010/011, REQ-001/006/010, SPEC-010/011,
IF-010/011, PKG-006. No vendor import, provider, media/cache operation, runtime
switch, source format conversion or producer execution. Rights/Provenance and
Editor checklists apply; producer review, not independent or creative approval.

The produced-claim adversarial tests found internally tagged unit enum variants
can ignore extra fields despite serde deny_unknown_fields. REEL has the same
shape in Payload::Selected: a selected selector with a stream slot must refuse,
rather than silently ignoring a requested language/representation field.
Change to an empty struct variant while retaining identical valid JSON wire
shape. Explicit named-stream selection remains separate. Rust callers must use
Selected {}; there is no deployed wire-format migration or fallback.

L0 fmt, all-target/all-feature warnings-denied Clippy, default-library independence
and diff. L1 reproduction then rejection matrix for malformed selected/stream
fields in Git and archive profiles, with valid selectors retained. L2 existing
synthetic native compile/context cases and exact-head Windows/Linux CI. The source
adapter pin changes truthfully; native frozen replay requires its matching adapter
binary as already specified. No historical selection or approval is changed.

Executed closeout: the new test first reproduced acceptance of a malformed
selected selector carrying slot=en. After the empty-struct correction, all22
normal Windows Tig consumer tests PASS (one FFmpeg case remains explicitly ignored
in this invocation), including both Git/archive malformed-selector matrices and
valid selected/named selectors with zero byte reads at source open. Workspace
all-target/all-feature warnings-denied Clippy, fmt, default-no-feature library
check and diff pass. Full current-head CI remains required. Rights/Provenance and
Editor producer findings: reject contradictory selectors rather than guessing;
preserve valid wire bytes and keep frozen adapter identity explicit. Disposition
pass_with_risk for this parser scope, not independent/human acceptance. No global
parser-completeness, runtime adoption, variant semantics or custody claim.
