# Derived comic issue controls

`comic-issue-control` is an additive deterministic workflow capability. Its
`reel.workflow-issue-control.v1` request names an authored issue plan and each
part's receipt/carrier paths, plus assembly byte limits. It contains no predicted
part hashes. Actual task inputs and upstream selectors bind the selected parent
results after those results exist.

The producer reads exact declared bytes, checks the master/edition/language,
authored page groups, explicit part IDs, carrier output identity and receipt
correspondence, then emits `assembly.json` and a technical worker receipt. The
assembly request pins the actual plan, receipts and carriers. Its logical output
identity includes the complete control task key. Changed parent bytes therefore
produce a new candidate; the original control request need not change.

Native dispatch uses existing owned workspaces, child-owned composite admission,
returned-file replay, retained-owner acknowledgement and historical native replay.
The final assembler consumes the derived request as an ordinary retained upstream
output alongside the retained parts. No new record is injected into an earlier
Git commit; this adds neither a registration transaction nor a cache engine.
The control producer checks correspondence, not embedded drawing renderability;
the selected native parent proof and final deterministic assembler own replay.

All 52 shared workflow tests and strict shared Clippy pass. Three added tests
check exact request generation and assembly binding, changed-byte/wrong-origin
refusal, and a stable recipe across unknown future part hashes. Native compilation
and strict Clippy pass; the expanded native fixture still needs end-to-end
qualification. It admits a separate explicitly enrolled control actor, retains
the control output, binds it into final assembly and exercises the existing
issue history/export/edit path. No actor is allocated implicitly.

Producer-applied comics/editor checks preserve authored grouping and page order;
rights/provenance checks bind exact inputs and keep technical completion separate
from creative, translation, font, print and release decisions. Existing 16-actor,
128-path and byte limits remain admission constraints. This producer does not
provide automatic graph expansion, general scheduling, slot retirement, candidate
selection or larger issue qualification. Those remain in the completion ledger.
