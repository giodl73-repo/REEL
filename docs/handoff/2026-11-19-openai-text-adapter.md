# Direct text proposals and structured reviews

`provider::Service::OpenaiText` uses the existing protected native HTTP dispatch,
original call identity and managed retention. It does not create another cache or
retry an uncertain call. The explicit service configuration is:

```json
{
  "kind": "openai-text",
  "spec": {
    "schema": "reel.openai-text.v1",
    "format": "text",
    "output": "candidate.txt",
    "max_output_tokens": 2048
  }
}
```

Set provider to `openai`, choose a model explicitly and declare the task as
`text-proposal`. Required outputs are the named `text/plain` candidate,
`text.json`, `http.json`, `provider-request.json`, `provider-response.json` and
`provider-config.json`; all companions are `application/json`. The candidate,
text metadata and HTTP headers have at most the existing 1 MiB control bound.
Transport evidence and workspace reservation use the existing explicit budgets.

The endpoint is `https://api.openai.com/v1/responses` with a named Bearer-secret
header. Literal loopback HTTP is available only through the existing explicit
synthetic/private bridge option. Foreign hosts, queries, fragments, credentials
in URLs and other API paths refuse. No model catalogue or entitlement is inferred
from a syntactically valid model name.

Requests preserve prompt/source Unicode, disable streaming, background work,
storage, tools and automatic truncation, and set an explicit output-token bound.
Only `send_inputs` are encoded. Exact bound UTF-8 plain text and JSON become
labeled text inputs; fully decoded exact PNGs become inline image inputs with
their artifact descriptors. Other media, including an entire film MP4 or audio,
refuse before network intent. Hidden native inputs remain checked locally.

Completed responses may contain reasoning items and exactly one completed
assistant message. Only output-text content is accepted. Refusals, incomplete
responses, tool calls, multiple messages, empty or oversized text cannot become
candidates. Exact returned text bytes are preserved without trimming or JSON
rewriting. `text.json` records the response ID, requested versus reported model,
request/response/spec/output hashes and false selection/approval flags; the full
private response preserves usage and other provider fields. A reported model is
service evidence, not independent authentication of the model or its quality.

## Structured review mode

Choose `format: review-report` for a `review-report` task and set `output` to the
report name in its exact `control/review-request`. The existing native review
binding builds the model-facing brief with original task/attempt, production
commit, language, unit keys, copy fingerprint and explicit allowed evidence.
The required request/copy/review inputs must be granted in `send_inputs`.
The report output uses `application/json`; the same five companions are required.

The Responses wire schema uses strict objects with every property required and
a fixed report schema/copy fingerprint. It intentionally leaves contextual unit,
frame, modality, evidence and edition checks to REEL's existing semantic validator
before import intent/candidate registration and again on retained historical
readback. A structurally valid model response cannot bypass these checks. The
current adapter supplies text/PNG review material only; video/audio inspection
and arbitrary custom JSON schemas remain separate work. The subsequent
[direct comic review qualification](2026-11-19-direct-comic-review.md) covers
native report retention/history/replay and contextual refusal in a bounded
synthetic comic case, including its documented original-operation recovery.

Request/response fields follow the [Responses reference](https://developers.openai.com/api/reference/resources/responses/methods/create)
and [Structured Outputs guide](https://developers.openai.com/api/docs/guides/structured-outputs?api-mode=responses),
checked on 2026-11-19. These describe the wire contract; synthetic tests do not
prove actual account/model access.

Applied rights/provenance and editor checklists: explicit private input egress,
source identity, language preservation, uncertain-call reconciliation and
candidate/selection separation. Text proposals and model findings are private
unapproved candidates. No real service call or creative/translation approval is
implied by component tests.

## Qualification

Source `11ac4d78569cc32330acb245eb4a120694f02a63` has all 1,908 tracked
files verified against its archived snapshot. All 27 HTTP tests, 61 standard
native tests, 247 unchanged SDK source pins and strict HTTP/native Clippy checks
(film and default features) pass.

The native text-proposal fixture passes in 142.32 test seconds / 143.08 wall
seconds: one synthetic local request, exact Spanish output, six retained outputs
and replay without another call or claim change. Its preserved operation root is
`F:/src/tig-dev-temp/.tmpu8woAj`, source `F:/src/tig-dev-temp/.tmpJhLJNI`.
The uncertainty fixture passes in 77.75 test seconds / 77.78 wall seconds: one
truncated local response, preserved request ID/received prefix, original Waiting
job and claims, and refusal to repeat the call even after temporary local evidence
loss. Its root is `F:/src/tig-dev-temp/.tmpPimgKK`, source
`F:/src/tig-dev-temp/.tmpB5syd2`. Neither operation is closed or scratch-retired.

External evidence: `F:/src/tig-dev-temp/reel-openai-text-20261119-v1/qualified-tools.json`,
SHA256 `ab18c761ef33b1a4e9532fd865eb667f650057800e95a55299d91642f82ab957`.
It pins source/archive, runtime/test executables, native logs, operation controls,
private call evidence and the HTTP test execution record. Both jobs retain their
original executable identity; uncertainty requires reconciliation of that exact
operation, not a new attempt or another POST.

This initial qualification covers text proposals on Windows/NTFS with the existing
synthetic single-pool fixture and literal loopback HTTP. The structured-review
wire/normalization tests alone do not establish native direct-review execution,
visual inspection, actual model access or project adoption. Subsequent bounded
native comic evidence is linked above; actual inspection/model/project scope
remains unqualified.
