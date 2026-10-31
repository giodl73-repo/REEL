# Keep output IDs distinct from filesystem names

A managed real episode reached its component worker and failed because a
language-scoped Tig output ID contained colons. The selected bytes and source
record resolved correctly, but independent master comparison incorrectly applied
the relative-filesystem-path contract to the adapter's logical asset ID.

`InputFiles::validate_reference_name` now declares the provider's namespace.
Its default retains strict bounded relative virtual paths. The selected-output
projection accepts prepared logical IDs; `verified_path` still requires the prepared exact
ID, hash, length and live byte guard. No logical ID is passed to filesystem join,
renamed, inferred from another record, or converted into a physical cache path.
Both scene and presentation master comparison use the provider's validation.
Control document paths continue to use their existing strict validation.

Both complete episode CLI fixtures use colon-containing output IDs throughout
capture, comparisons, assembly, receipt and temporary-owner handoff, with the
source checkout and original renders offline. A filesystem-provider test rejects
colons, traversal, absolute paths and empty path components. Existing stale,
missing, changed-byte and ownership checks remain part of those fixtures.

Producer-applied rights/provenance review: output references remain pinned to
their original source and selected object; no namespace fallback, upload,
creative approval, consumer binding or eviction authority is added. The failed
real attempt retained diagnostics and its complete input context, released all
24 temporary component sources, and produced no parent output. These are
technical findings, not independent approval or a completed consumer cutover.

Local qualification passed strict locked all-target/all-feature Clippy, the
filesystem guard regression, and both complete CLI fixtures in 288.46 seconds.
The first actual retry rejected the historical composition context before job
adoption because that context pins the input adapter's implementation bytes.
A new capture under the tested executable reopened all 24 controls exactly and
adopted no job or media. The real retry uses that capture; refreshing a compiler
context does not revise source records or approve previously selected outputs.
Actual assembly and hosted qualification remain separate acceptance results.

Hosted original-producer coverage caught an unnecessary input-compiler context
change: placing the output namespace override on `tig_inputs::Inputs` changed
the source bytes pinned by the input interpretation contract. The override now
belongs to a selected-output projection shared by scene, presentation ownership
and standalone presentation comparison. The primary input adapter is byte-for-
byte unchanged from the qualified base. Historical compiler checks remain strict;
no original claim is reissued under a new input interpretation.
