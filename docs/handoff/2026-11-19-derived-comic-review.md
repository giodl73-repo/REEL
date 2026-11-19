# Retained comic-to-review control production

`ComicReviewControl` is an owned deterministic workflow producer. It consumes an
exact ComicRender dependency, including its worker receipt, complete selected
inputs and outputs. The existing composite protocol verifies retained parent
bytes before first child admission; the child then acquires its own claims.
The generator requires live child authority and frozen origins. Deserialized
parent metadata or an unadmitted reader cannot grant that authority.

The pure generator binds dependency/result/receipt identity, producer publication,
edition/language, every source alias and the complete declared input set. It reads
exact bytes, checks original comic source/receipt correspondence, then produces:

- `review-copy.json`: existing page/panel/balloon targets and selected page order;
- `review-sources.json`: ordered reading PNG descriptors;
- `review-request.json`: a pinned structured-review request naming the copy and
  reviewed production commit; and
- a separate deterministic worker receipt binding parent and child identities.

The first three are required JSON outputs of at most 1 MiB each. The request's
logical identity is `control/review-request`, matching the structured operation.
Copy/source identities include the child task key. Their hashes are outputs,
never inputs to their own task key, so no circular Git or output pin is needed.
No new Git record or production commit is required to pass the retained outputs
into the next task. Original producer inputs still resolve against their exact
commit. Selected-part controls describe only the selected pages.

The native worker uses existing staging, deterministic replay, candidate ingress,
retained proof and journal completion. Historical replay uses the child's own
frozen origin archive and retained closure, without reopening the original source
actor generation. It checks the exact previously admitted producer lineage; it
does not rerender or reapprove the original comic. Source/output byte gates drop
before managed retention effects. Existing 16-actor, 128-path/input and 64 MiB
snapshot/upstream limits remain admission limits; this adds no cache or allocator.

Qualification passed in 884.54 seconds at source `0331d3b`. The existing-pool
fixture explicitly closes the original retained comic slot, reuses it for automatic
review-control dispatch and completion/replay, closes that slot, and independently
admits/stages a structured review from retained control and reading dependencies.
The review remains Waiting for intelligence at `.tmp1KAuie/review`; its protected
prompt batch is `dispatch/ready.json`. The original `.tmpgipRl8` producer and the
derived-control source generations are closed. The review owns the reused slot.
Original close tickets, work identities, activation generations, profiles,
journals, reservations and scratch are kept. No production commit changes.
If interrupted, reconcile those original controls rather than restart the fixture.

All 83 workflow tests, 57 default native tests, the 247 SDK source pins and strict
Clippy for workflow/native with and without `film` pass. Formatting and
`git diff --check` pass. All 1,882 archived source files match. The fixture uses a
32 MiB Rust test-thread stack and the existing bounded-stack native CLI. Exact
tool/source, control and log pins are recorded by
`F:/src/tig-dev-temp/reel-derived-comic-review-20261119-v1/qualified-tools.json`
(SHA256 `1290904b7aa90b2215e85e3cad0c7648bac64a05c66745e7eea572358647ecbf`).
The measured time includes repeated complete native parent/child proof; it is a
correctness qualification, not a large-issue throughput claim.

Producer-applied editor, comics editor and rights/provenance review preserves
source wording, language/version isolation and full contextual review. Technical
retention or a staged prompt grants no creative, translation, font, selection,
print or release approval. No independent review is claimed.

This capability covers ComicRender and its selected-part contract, not complete
issue-assembly or film review-control production. Larger issues, film lineage,
returned composite-review completion, actual providers, issue effects, repairs
and evidenced closure remain separate qualification work.
