# Direct Images API candidates under the protected workflow

`reel-workflow-http::openai_image` composes bounded JSON requests and checks PNG
responses. Native provider configuration adds `service.kind: openai-image` for
`ImageCandidate` tasks with provider `openai`. The configured model is explicit;
the adapter supplies no default model, discovery, entitlement claim or fallback.

The wire implementation follows the official [generation reference](https://developers.openai.com/api/reference/resources/images/methods/generate)
and [JSON edit reference](https://developers.openai.com/api/reference/resources/images/methods/edit),
checked 2026-11-19. Generation uses `/v1/images/generations`; a nonempty reference
list uses `/v1/images/edits`. Edit references and optional masks use data URLs of
exact declared PNG bytes. Requests select one non-streaming PNG, explicit size,
quality and background. Model-specific size/feature access remains a service
constraint; this bounded adapter does not maintain a live capability catalogue.

Service configuration fragment:

```json
{
  "kind": "openai-image",
  "spec": {
    "schema": "reel.openai-image.v1",
    "references": ["art/character-reference"],
    "mask": null,
    "size": [1024, 1024],
    "quality": "high",
    "background": "opaque"
  }
}
```

Place this in the existing native provider config's `service` field. Set its
`provider` to `openai`, `model` to the explicitly chosen GPT image model, and HTTP
endpoint to the corresponding canonical `https://api.openai.com/v1/images/...`
path. Configure the Authorization secret header with prefix `Bearer ` and an
explicit environment-variable reference. Credential values never enter retained
config/request JSON. Synthetic qualification allows literal loopback HTTP only;
arbitrary hosts, query strings, credentials in URLs and wrong service paths refuse.
Normal proxy configuration remains available through its existing protocol.

`send_inputs` must equal the exact reference/mask identity set. Every referenced
PNG must be a declared task input; its length, hash, full decode and bounded layout
are checked before constructing the request. Reference order is preserved. A mask
must have an alpha channel and match the first reference's dimensions. Unrelated
local inputs are not appended to the API request. The task's authored prompt is
sent exactly, without translation, extra private context or inferred instructions.

The task declares exactly six required outputs:

| Name | Media type |
| --- | --- |
| `candidate.png` | `image/png` |
| `image.json` | `application/json` |
| `http.json` | `application/json` |
| `provider-request.json` | `application/json` |
| `provider-response.json` | `application/json` |
| `provider-config.json` | `application/json` |

The existing dispatcher accepts input protection, stages owned inputs, persists
original call ownership/intent before HTTP and preserves response headers/prefixes.
Only a completed successful response can normalize into candidate files. The
existing native pipeline ingests, protects, publishes, reopens and acknowledges
retained ownership before journal completion. Replay uses the original request,
response, ticket and operation; an uncertain call never becomes another POST.
The client operation header is correlation data, not a claim of provider-side
idempotency or a remotely queryable image job. A lost reply can remain unresolved.

## Validation and bounds

Only inline base64 output is accepted; no returned URL is fetched. Exactly one
result must decode as a complete non-animated PNG, with checked dimensions and
bounded decompression. Requested transparent output requires an alpha channel;
actual transparent-pixel presence is recorded separately. Opaque output refuses
transparent pixels. A PNG, base64, layout or context error preserves the original
response and refuses completion; it does not select a different response or retry.

Bounds include sixteen references, 32,000 prompt bytes, data URLs within the JSON
API length bound, 128 MiB transport ceilings, 64 MiB PNG/decoder-buffer ceilings
and at most 16,777,216 decoded pixels. Output requests have axes divisible by 16,
through 4,096, at most 8,294,400 pixels and aspect ratio between 1:3 and 3:1.
These are adapter admission limits, not assurances that every model accepts every
combination. Native object/workspace/job bounds remain independent.

`image.json` records the requested model, prompt/spec/response hashes, observed
image dimensions/alpha and candidate hash. It does not authenticate the actual
backend model. Raw response preserves provider revisions/usage and other fields;
the compact metadata does not copy revised prompt prose. All six outputs are
private workflow evidence, not shareable receipts. Candidate bytes are neither
selected nor creatively approved by normalization or retention.

The direct adapter's service-specific output/evidence contract is not a manual
image-pack normalizer. Manual handoffs retain their existing task/result contract;
they must not fabricate an HTTP trace or API provenance. Native reference/mask
request construction and retained execution are qualified separately from actual
account/model access, model behavior, likeness/reference rights and release.

Applied rights/provenance and animation-direction producer checklists: explicit
input egress, pinned source bytes, private evidence, bounded decode and no automatic
selection. No real API call or private reference upload is required for the
synthetic qualification fixtures.

## Qualification

Source `2fcaa3413322caed191eadf343cb2ca57f0c94e8` adds the adapter;
`9d3af1f0617202eb937e329b2baf51efcd407133` fixes a shared test helper to
repin the supplied task ID. The initial native fixture stopped in that helper,
before admission, publication or HTTP; its failure/source logs remain preserved.
No accepted operation was restarted under another identity.

There are 23 passing HTTP tests, including five image cases covering generation,
ordered references/masks, endpoint/credential restrictions, invalid outputs and
full 16-bit alpha precision. The native regression suite passed 59 tests, including
egress/operation/output refusal and all 247 SDK source pins. Native standard checks
preceded the final PNG16 precision refinement/helper repair; that refinement has
HTTP regression coverage and the final native execution below. Strict HTTP and
native Clippy checks pass. All 1,900 archived final source files verified.

The protected loopback edit fixture passed in 207.11 test seconds (207.77 wall),
with one exact reference PNG, one request, a checked 1024×1024 candidate, six
retained outputs and replay without another POST. The truncated-response fixture
passed in 80.02 test seconds (80.04 wall): it retained the request ID/prefix,
preserved claims and refused another call, including when local call files were
temporarily moved within their owned workspace. The uncertain job remains Waiting.
Generation and masks are component-qualified; native execution qualification is
the single-reference edit path. No real service account/model was contacted.

Preserved roots are `F:/src/tig-dev-temp/.tmpIrOFpr` (positive) and
`F:/src/tig-dev-temp/.tmpJXuW6J` (uncertain), with their original source/workspace
paths pinned in the evidence. The combined source/tools/checks/operation manifest is
`F:/src/tig-dev-temp/reel-openai-image-20261119-v2/qualified-tools.json`,
SHA256 `f3c606b404d0572dba3510395902778d8c147ea5d70d71ba636c4f718cee2549`.
