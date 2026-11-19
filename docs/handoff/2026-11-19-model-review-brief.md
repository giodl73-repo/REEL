# Model-facing review briefs and result packing

`review::Bound::brief` now supplies the same private review contract to manual
handoffs and generic proxy calls: exact task/attempt/copy identity, valid unit
keys, allowed evidence descriptors, format-specific modalities, a return-schema
guide and an empty report template. The instructions preserve the authored
prompt and source wording, require actual review modality, and separate report
JSON from the result envelope that Rust creates. An empty template proves no
viewing and grants no creative or release approval.

Evidence in a proxy brief is restricted to `send_inputs`. The request, selected
copy and selected review artifact must be explicitly allowed; missing permission
refuses before a POST. Other staged descriptors are omitted. Selecting a comic
manifest does not itself send its reading images: the adapter must explicitly
include the media it will inspect. Native semantic checks still enforce exact
pins, byte limits, frame containment and evidence identities; the schema guide
is not a replacement for those checks or a provider-specific dialect adapter.

Structured-review proxy requests use `reel.workflow-proxy-request.v2` and add a
`review_brief` field. Legacy requests remain byte-identical v1, and timed speech
uses its existing service request. Proxy implementations should consume the
brief instructions/schema for the new operation and normalize the report into
the existing named-output response protocol. Artifact content remains data,
never execution instructions.

Two native commands expose the manual path:

```text
reel-workflow-native export-review-brief PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT NEW_BRIEF --apply
reel-workflow-native pack-review-result PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT REPORT NEW_RESULT --apply
```

Both reopen the original active input claims and exact request/copy. Brief export
is a read-only supplement; it does not rewrite old ready batches or runtime pins.
The packer validates report semantics and hashes the exact file bytes, including
whitespace, into a Handoff result under the admitted attempt. It invents no
provider/model provenance. It handles a single report body and refuses tasks
with other required outputs rather than discard them. Generic proxy execution
keeps its existing multi-output/evidence normalization.

Output controls are private, bounded and exclusively created. A stale report
refuses before creating its result file. Write failure preserves partial controls.
Neither command registers a candidate, publishes/retains a result, completes a
task, selects an asset, files an issue or releases claims. Import/recovery still
uses the original pinned lifecycle runtime and existing returned-file bindings.

Qualification passed in 30.55 seconds at source `43929b3`. The fixture supplements
the previously staged Waiting review in `.tmp1KAuie/review` using a new read-only
frontend, packs an empty technical fixture report, rejects a stale report before
output creation, refuses overwrite and verifies unchanged claims and Waiting
journal. Its preserved root is `.tmp4vUhZd`. It makes no provider call or
creative-review claim, and does not retarget the original lifecycle runtime.

All 86 workflow tests, 58 default native tests, the 247 SDK source pins and strict
Clippy for workflow/native with and without `film` pass. Formatting and
`git diff --check` pass. All 1,884 archived source files match. Exact tools,
source, control and log pins are recorded in
`F:/src/tig-dev-temp/reel-model-review-brief-20261119-v1/qualified-tools.json`
(SHA256 `090391f7d8415d4ad9b0f0ea4f0d2e4b80fa822238d235f2e608a4900f780385`).
This is bounded Windows/NTFS manual-brief/packing qualification. Proxy v1 byte
preservation and v2 extension are component-tested; no live v2 proxy/model call
or returned-report import is established by this fixture.

Producer-applied editor, comics editor and rights/provenance review: exact
language/version and evidence stay explicit; actual viewing/listening and human
authority remain separate. The source prompt and private context are not
shareable public issue bodies. No independent review is claimed. Real v2 proxy
service/model qualification, returned composite-review completion, issue effects,
repairs and evidenced closure remain open.
