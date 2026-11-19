# Protected native provider proxy

`dispatch-task` and optional recipe execution now accept a `provider` configuration
with schema `reel.workflow-provider-proxy.v1`. It names `provider`, `model`, ordered
`send_inputs`, an existing `reel.workflow-http.v1` transport configuration and a
`workspace` staging/capacity profile. Deterministic workers and provider calls are
mutually exclusive. Omitting both keeps the existing manual handoff behavior.

The task must declare three required JSON evidence outputs:
`provider-request.json` and `provider-response.json`, each allowing the configured
transport byte bound, and `provider-config.json` allowing the 1 MiB control bound.
These preserve exact sent/received bytes and effective configuration, including
model, endpoint/policy and credential environment references without resolved
credential values. They become ordinary unselected outputs under actual native
publication/retained acknowledgement; request execution alone is not completion.

The request body uses `reel.workflow-proxy-request.v1`: an original operation ID,
provider/model, task identity, attempt, operation, language, prompt, creative output
contracts and ordered explicitly selected inputs with exact artifact descriptors
and base64 bytes. Source Git locators, native bindings and unrelated input bytes
are not sent. All declared inputs must still reopen through the actual admitted
native reader; staged copies cannot substitute for unavailable native bytes.

The proxy replies with `reel.workflow-proxy-response.v1`, echoing `operation_id`,
`provider` and `model`, plus an `outputs` map of creative output name to
`{ "media_type": "...", "base64": "..." }`. Evidence names are reserved.
Unknown/duplicate/excessive outputs, mismatched context/type, oversized or invalid
base64 refuse. This version uses flat portable output names. The adapter computes
exact artifact hashes and builds the Direct-mode result; no returned code runs.

Preparation uses existing owned-workspace/capacity admission. A one-owner marker
per original native admission is exclusively created and flushed under the
existing coordination root before the local call intent and network execution.
Another workspace/config cannot bypass that marker. A global marker without the
original local call witness is uncertain and cannot launch another call. Owned
identity, original admission, pinned batch/runtime, request hash and HTTP config
hash are checked before response reuse. No actor allocation or new cache engine
is introduced.

Received response prefixes are flushed into the original workspace. Complete HTTP
status/length/hash metadata is stored separately. Timeout, truncation, missing
completion metadata and non-success status require original-operation
reconciliation; they never cause another POST. Complete captured replies can
deterministically rebuild missing final control files and resume original native
ingress. Partial conflicting files are preserved/refused. Generic service-side
lookup or adoption of an externally reconciled incomplete call is still open.

All source/native input gates are dropped before native candidate ingress.
Completion uses the existing retained-result path and independently verified
retained coverage. Dispatch/recipe summaries report whether this invocation
actually called the provider. Manual pending returns remain explicit; provider
pending ingress can resume the original captured result.

Preflight/native-missing-input tests prove no network effects or new claims.
Both existing-pool fixtures pass: one local HTTP call followed by retained
evidence/replay (174.82 seconds), and uncertain response preservation, unchanged
claims, no repeated POST and refusal after loss of local call files (66.72 seconds).
All 48 default native tests and strict native Clippy pass. They use synthetic data
and an explicit existing native pool, not real providers or private media.

Evidence: `F:/src/tig-dev-temp/reel-workflow-native-provider-20261119-v1/qualified-tools.json`.
Source pin `4e72808`; all 1,838 archived files match. Runtime SHA256:
`8b449aef6d80c32bea0586510603f52aee381568b8f70d9c50ddb5fe7036d267`.
Fixture SHA256:
`97d5f7c44fcf4fee4b98fe4139660f9666f43bef54317afca7ea430cd64823bd`.
The complete controller is `F:/src/tig-dev-temp/.tmpcgbNWP`, source
`F:/src/tig-dev-temp/.tmpIbeYJ3`; its result is Retained. The uncertain controller
is `F:/src/tig-dev-temp/.tmpF6U0Cj`, source `F:/src/tig-dev-temp/.tmpX85dfp`;
its original job remains Waiting. Preserve both contexts and scratch, especially
the uncertain call owner's record, original ticket and captured response prefix.

This is the generic proxy protocol. Built-in service-specific text/image and
ElevenLabs v4 request composition, account/model/voice admission, contextual
creative review and project/complete comic issue adoption remain separate.
Producer-applied recovery/custody and privacy/provenance checks do not grant
independent review, consent, candidate selection or release authority.
