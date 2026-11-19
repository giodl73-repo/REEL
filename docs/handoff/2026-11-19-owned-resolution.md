# Owned native resolution evidence

`ToolCapability::RepairResolution` produces `resolution.json` and a separate
worker receipt through the existing deterministic worker, candidate ingestion,
native publication, retained acknowledgement and replay lifecycle. It adds no
cache engine, provider call, issue close, selection or scratch retirement.

```text
reel-workflow-native export-resolution-request BASELINE_RETAINED_REVIEW PLAN REPAIR REVISION REBUILD_BINDINGS CORRECTED_RETAINED_REVIEWS DECISION NEW_REQUEST --apply
```

The private `reel.workflow-resolution-job.v1` request freezes the original
baseline/corrected review bundles, repair controls and observed retained producer
contexts. Capture checks immutable profile pins, journal result/acknowledgement
identity and exact source snapshots. It does not grant custody or complete a job.
Machine profiles/locators remain private payload data, not authored Git records.

`Request::resources` enumerates every declared parent native payload (including
unused record streams), result artifact and worker receipt. Object aliases use
hash/length with an explicit custody-only media type; the original record/annex
leaf identity is independently checked by the original retained proofs. Content
deduplication is not used to substitute an original asset identity. All contexts
and the child must use the same explicitly declared native store and physical
pool. Original contexts, Git history and pinned verification tools must remain
available; missing history is an error, never a latest-context fallback.

The task declares the request as `control/resolution-request`, the baseline plan
as its distinct publication input, and the complete resource alias set. Its tool
has no auxiliary programs/parts, resource `source_bindings`, a pinned native
runtime and one required JSON output bounded by 1 MiB. Native admission owns
durable claims for the full declared closure before dispatch. The logical
126-resource/512 MiB bound does not override the SDK's 128-change transaction or
16-actor limits; preview/admission may refuse earlier. There is no splitting or
automatic alternative store.

Production rechecks frozen contexts, source plan, input set and source bindings,
then recomputes the checked resolution with exact producer-derived review copies.
It requires live admitted or retained child authority. Captured control bytes
are hash-checked; their read gates are released before reopening parent proofs
while durable child fences remain held, avoiding reentrant payload gates when
control bytes are also parent inputs. Resource aliases themselves are protected
by the child and checked during staging; parent proofs own their read gates.

The receipt and every output must equal deterministic replay before native
completion/history is accepted. `RetainedGuard::resolution_proposal()` returns
the checked typed proposal only from the live resolution proof. Deserializing
JSON or an acknowledgement cannot create that guard or grant approval.

The native fixture uses the existing corrected-comic lineage at `.tmpkHoGyv`,
admits a new owned resolution task, checks each resource's protection, dispatches
through the existing worker, retains/replays it and reopens the typed native proof.
It compares against the earlier qualified local proposal and preserves all three
parent claim files. The fixture's actual qualification results are recorded only
after execution; compilation alone does not establish lifecycle completion.

Rights/provenance and comics-editor checks preserve original evidence, exact
language/context, private metadata and technical/creative separation. No actual
viewing, creative approval, tracker effect or public release is implied. Retained
tracker delivery/close receipts and managed close scheduling remain separate work.
