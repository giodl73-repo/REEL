# Asset migration tool integration

Integrate private review source `188aab8cbef65ce5f9e336097dcfa36bfeffdc3a`
and source-preservation source `571322ce7519e3b282c58318532addbcaab62c6f`.
The qualified parent `32f21dd45dbf59726c7286494a080e826ee2c2aa` and merged
main `1b13cbd549a0fdba90bb907b2b928195b827992c` have identical complete trees.
An ancestry-only merge preserved the entire private-review tree before the
source-preservation commit was applied. No independent main change was discarded.

The [private-review contract](../delivery-private-review.md) preserves exact
permission and non-consumed job evidence. The [source-preservation review](2026-11-01-version-label-slots.md)
covers lossless long labels and incremental history. Both address actual asset
migration gaps without changing installed production bindings or introducing a
second cache engine.

On the combined tree, all eleven versioned-registration cases, four package
cases and twenty regular delivery-template cases pass, as does locked all-target/
all-feature Clippy with warnings denied. The two local managed native cases were
executed before the source-preservation integration; their original source and
logs remain separate. The final hosted workflow explicitly runs both native
cases on the integrated commit. Actual project production, custody and full
consumer cutover remain open.

Producer identity/storage/privacy review keeps original execution attribution,
retained subjects and source statuses distinct from technical qualification.
This is integration evidence, not independent review or creative approval.
