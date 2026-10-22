# Source-authority and delivery reference adapters

Strict generic source-authority and delivery records are now part of the Rust
reference inspector and generated schema catalogue. Scenes bind their source
authority by explicit profile mapping; declared cue/presentation scopes must be
covered. Delivery records lock an immutable version and name retained outputs
plus opaque evidence. See [the contract](../asset-references-v1.md).

These are reference records, not a second source grammar, authorization engine
or delivery client. Their scopes, audience, output role and language are claims.
Source content, evidence contents and remote bytes are not verified; every record
retains the applicable unverified-semantics finding. Evidence is never parsed by
guessing its schema. No cache/archive crawl, daemon, global lock or mutation.

Synthetic tests and producer review: [review](../reviews/2026-10-21-authority-delivery-references.md).
The asset repository owns compact declarations and exact private evidence,
including historical receipts. It must not rewrite those immutable receipts to
make a graph pass. Old saved reports stay readable; fresh scene inspections now
surface source-authority links previously omitted. Bind them explicitly or keep
the unresolved finding. Next: source-text/cue ownership and scoped evidence
interpretation, with remote delivery verification as a separate explicit action.
