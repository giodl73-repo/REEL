# Bounded native preparation before CLI adoption

Entryda165c8ecc1831bb51324ca020cc2767a2f6ac1d (REEL207 dependent stack).
Adapter contribution to Tig WP-010/011, IF-010/011. Code trace for CLI context-mode
adoption found native preparation writing controls/contracts before worker capacity
admission. Existing render-context contract size checks occur after compilation.
First enforce limits during writes; do not introduce unreserved CLI staging.

Scope: one shared in-memory logical write budget across native control copies,
serialized native metadata and the six generic compiler outputs. Native preparation
allows64 MiB total and4 MiB per document. Bounded serialization refuses before
creating the offending file, during serialization rather than after building the
entire rejected document.
Keep existing unbounded generic compiler APIs and valid output bytes unchanged.
Budget every native path, including Sonic evidence compilation. Do not reset the
budget between the adapter and compiler. Partial scratch remains caller-owned.

This bounds logical file bytes, not filesystem allocation, heap usage of the
already-built semantic IR, physical capacity, source custody or runtime admission.
CLI adoption still needs explicit workspace reservation/accounting, including
multiple preparation passes, allocation overhead and cleanup. No new cache,
daemon, media render, runtime binding or CLI default switch.

L0: format/lint/default isolation. L1: per-document, total and serialization
limits before file creation, shared allowance across serializers/raw writes,
failure without truncating existing files, unchanged generic compatibility.
L2: native scope/recipe/job regressions and opt-in retained English/Spanish
contract/recipe replay under the new bounds. Exact-head CI and ancestor integration
precede squash merge. Producer editor/sound/rights review preserves exact valid
contract bytes, original pins and caller ownership; no claim of physical capacity,
complete freshness, approval or eviction. Roles are producer checklists only.

Executed evidence: three new unit cases pass for shared raw/compact/pretty
allowance, exact valid serialized bytes versus the generic writer, refusal before
file creation, exhausted total/document allowance, existing-file preservation
and bounded buffer length on rejected writes. Final native integration47 cases
pass with7 existing ignored source/external cases. The native adapter passes the
same budget through both generic compiler branches, including guarded evidence;
it does not reset the allowance at the compiler boundary.

Retained Melena c22fcce91968cd944dab271de96809d1092a5923 English and Spanish
projected recipe rehearsals pass after metadata finish/reopen with29 primaries,
5 controls and30 roots each. Only named controls/alignments are read. No retained
media rendering or production cache write. Final Rust sources are unchanged
after this qualification; the normally ignored rehearsal remains explicit.

All-target/all-feature warnings-denied Clippy passes. Separate no-default-feature
library Clippy with warnings denied passes; default library189 tests pass with9
existing ignored cases. Format/whitespace checks pass. This turn does not repeat
the external FFmpeg fixtures; valid-byte comparison, complete native regression
and actual contract/recipe replay qualify the writer changes locally. Exact-head
CI still runs the external cases before integration.

Producer editor/sound/rights disposition: pass_with_risk for logical preparation
bounds. Physical allocation/reservation, interrupted-workspace recovery and CLI
context-mode selection remain subsequent work. No new claims about global
coverage, generated-input freshness, complete environment or custody.

Parent integration:204 exact18050ba7 passed all3CI and merged squash to
9a967e472dd178a9f5fa81ed46abf7bd28d11d00.205 was rebased onto that main, dropping
oldf496223; rebased2aad9fa637de69738a4d9eba1b078a70d13f589b has exactly the
old300a9d4 source tree. Its old CI was terminal/successful before explicit leased
push; fresh exact-head qualification is required. No live CI was cancelled.
