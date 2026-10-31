# Generic frozen-job capture review

Scope: explicit Rust metadata capture for the `run-frozen` admission introduced
in PR 272. The existing committed-source adapter, context/recipe/job APIs,
metadata store, capacity reservation and contention handling remain authoritative.
Only the metadata allowance helper's visibility widens within this crate.

Producer-applied Git/adoption, concurrency/recovery and rights/provenance lenses:

| Requirement | Finding / mitigation |
| --- | --- |
| Source meaning comes from external admission | Root, expected checkpoint, pinned definitions, graph profiles and consumed authored fields are explicit; stored status grants no authority |
| Capture must not silently initialize storage | All stores, cache roots, coordination and output parent must already exist; missing-store fixture refuses without creating it |
| Metadata growth must be budgeted | Reuse existing database/rollback/output allowance on the explicitly enrolled cache volume; keep reservation through finish/readback and fragment publication |
| Metadata concurrency must not block all workers | Reuse the native capture's short provider-specific retry; no new cache-wide lock |
| Candidate roots need independent verification | Finish/reopen store, replay the source context and exact job admission before no-replace fragment publication |
| Capture must not imply protected job execution | No source journal open/adopt, media read, renderer, output retention or production assertion; source transitions remain explicit |
| Existing output must remain intact | No-replace publication; repeat capture refuses and preserves the fragment |
| Partial failure must not revoke old roots | Possible deduplicated metadata additions grant no success/ownership; existing source remains protected externally |
| Private evidence must remain private | Fragment contains local admission/pins; fixed public summary/errors omit source identity, paths and text |

The managed fixture uses an empty second metadata store and independently
constructed expected roots. Actual CLI capture reproduces both roots without
the old selected media, then the existing CLI runner consumes that captured
store, retains control and record, and creates the isolated one-record proposal.
Six altered capture admissions refuse before metadata mutation; capture leaves
the source journal unchanged. Existing worker and run-admission refusals remain.

These are technical producer findings, not independent or creative approval.
Actual project capture/enrollment, consumer adoption, current render checks and
production runtime installation remain separate acceptance work.
