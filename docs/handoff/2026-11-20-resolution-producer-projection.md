# Exact producer correspondence for derived review manifests

The actual retained corrected issue exports its manifest under a producer-scoped
logical ID. Its retained review-control task emits the same bytes under the
control's own logical ID. Native resolution previously compared entire copy
descriptors strictly, refusing this legitimate derived namespace even after a
v2 report proved the exact producer chain.

The resolution adapter now permits that logical-ID projection only when the
retained report's bound witness equals the independently observed producer's
complete packet and returned result. Every other copy field must still match:
manifest hash/length/media, publication, source commit, language, format, ordered
pages and all units. Unbound legacy reports retain their strict descriptor
comparison. Matching bytes without the authenticated native report and producer
proofs confer no correspondence or custody.

Production evidence records the actual reviewed copy fingerprint and, only for
a derived namespace, an additional producer-copy fingerprint. Legacy equal-copy
evidence omits that optional field and retains its previous serialized bytes.

An opt-in read-only qualification uses the actual retained v2 report and original
issue, verifies native histories and tests changed manifest hash, length, media,
publication, language, page order, units, producer attempt and packet refusals.
It must also refuse the namespace projection when the witness is removed and
leave actual Index unchanged. This fixture has not yet run. It emits no resolution
proposal, review declaration, finding closure, selection or approval.

Producer-applied comics-editor and rights/provenance checklists require whole
issue context and exact immutable lineage. They are not independent reviews or
authority to infer viewing or resolution from an empty report.
