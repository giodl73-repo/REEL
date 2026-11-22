# Bounded workflow API/proxy execution

This isolated crate sends explicit JSON POSTs to an API or API proxy. Its Windows
CLI pins the private request, owns a new workspace, reserves capacity through
REEL's existing ledger and preserves original-operation evidence. Its portable
library provides transport and the workflow proxy wire contract. Producer and
native SDK dependencies/lockfiles are unchanged.

```text
reel-workflow-http capabilities
reel-workflow-http post REQUEST REQUEST_SHA256 --apply
reel-workflow-http resume REQUEST REQUEST_SHA256
```

`post` is an effect. `resume` reads and reconciles the original captured response;
it never sends HTTP, reloads source input files or needs API credentials. Missing
or partial response-completion evidence refuses with `NeedsRecovery`. There is
no automatic retry or cleanup. Keep the original request, registry, operation,
workspace, capacity entry, response prefixes and service correlation headers.
An uncertain call must be reconciled with its service before any new effect.

## Private request

Pin the exact raw request SHA-256 externally. Both the existing registry and
workspace parent must be caller-owned private ordinary Windows directories,
with neither inside the other. No native pool or cache is initialized. Use the
same registry for every retry of the operation; changing registry is not recovery.

```json
{
  "schema": "reel.workflow-http-call.v1",
  "http": {
    "schema": "reel.workflow-http.v1",
    "endpoint": "https://your-authorized-provider.example/api",
    "timeout_seconds": 300,
    "request_byte_limit": 1048576,
    "response_byte_limit": 33554432,
    "secret_headers": {
      "authorization": { "variable": "PROVIDER_API_KEY", "prefix": "Bearer " }
    },
    "allow_loopback_http": false
  },
  "payload": {
    "kind": "json",
    "operation_id": "original-call-1",
    "body": {
      "path": "X:/private/body.json",
      "sha256": "<exact body SHA-256>",
      "bytes": 123
    }
  },
  "registry": "X:/private/coordination",
  "workspace": {
    "parent": "X:/private/scratch",
    "maximum_bytes": 134217728,
    "capacity_policy": {
      "minimum_free_bytes": 1073741824,
      "minimum_free_basis_points": 500,
      "max_object_bytes": 268435456,
      "tier_minimum_free_bytes": {}
    }
  }
}
```

Replace the illustrative paths, byte count and hash before execution. The body
is exact caller-authored API JSON; its service/model/version and semantics are
the caller's responsibility. Secrets resolve only from named environment
variables. Request and response limits are each 1–128 MiB; deadlines are
1–21,600 seconds. The workspace budget is 8 MiB–1 GiB and must cover the encoded
request, three response allowances and 8 MiB of owner/control headroom. Capacity
remains reserved on success or failure; release/retirement is separate work.

The registry's exclusive operation record binds the raw request, parent/root
filesystem identities, reservation and volume. Another `post`, even with a new
workspace/config, cannot reuse that operation in the original registry. Partial
preparation is preserved for explicit diagnosis, never adopted automatically.

The original `http-<operation>` root contains the frozen invocation and request,
call intent, observed response headers, captured response and completion record.
Resume verifies their exact bindings before accepting any captured result. It
does not turn an uncertain response prefix into a completed response.

## Workflow proxy mode

Replace `payload` with:

```json
{
  "kind": "proxy",
  "spec": {
    "packet": "<existing workflow packet object>",
    "attempt_id": "attempt-1",
    "provider": "your-provider",
    "model": "your-pinned-model-version"
  },
  "inputs": {
    "declared/logical/input": {
      "path": "X:/private/explicit-input.bin",
      "sha256": "<exact declared artifact hash>",
      "bytes": 123
    }
  }
}
```

`packet` is an object, not the illustrative string. Only explicitly named inputs
are read and base64-encoded; each must match a declared artifact's identity,
length and content. Unsent input metadata remains part of the private packet.
The endpoint receives `reel.workflow-proxy-request.v1` with `operation_id`,
`provider`, `model`, `attempt_id`, the full packet and named encoded inputs.

Return the following bounded JSON, using the request's exact operation/provider/
model and the packet's output names and MIME types:

```json
{
  "schema": "reel.workflow-proxy-response.v1",
  "operation_id": "<operation from request>",
  "provider": "your-provider",
  "model": "your-pinned-model-version",
  "outputs": {
    "note.txt": { "media_type": "text/plain", "base64": "aGVsbG8=" }
  }
}
```

Proxy operation identity depends on task key and attempt, independently of model,
endpoint or workspace. A changed config cannot silently cause another call for
that task attempt. Duplicate, undeclared, missing required, malformed, wrong MIME
or oversized outputs refuse. Optional outputs may be omitted. Names must be flat,
portable and distinct ignoring case. Total decoded outputs are at most 128 MiB.
Deterministic tools, external source intake and new speech candidates require
their own adapters and refuse before a proxy call.

On a successful, valid reply the owned root receives `outputs/`, `result.json`
and `returns.json`. The result is the existing workflow `ReturnedResult` in direct
mode; it passes the same packet, attempt, contract and byte checks as a manual
candidate. Provider/model labels are reported provenance, not authenticated
service identity. Resume can restore missing metadata from the exact captured
response; changed or partial existing files are preserved and refused.

These files can enter the landed native `ingest-response`, `retain-response`
and `verify-retained` commands using the original packet/source/admission/receipt.
If inputs need native protection or export, perform the existing native admission
and owned staging steps **before** posting. This transport does not establish
those claims or write native task completion. Native protection remains live
until its explicit retained-owner close; HTTP success never closes it.

## Transport and qualification

HTTPS certificate validation stays enabled. Optional HTTP permits literal loopback
IPs only. Redirects, forwarding proxies, decompression, HTTP/2 and automatic
retries are disabled. In particular the pinned client's
[`retry::never`](https://docs.rs/reqwest/0.13.5/reqwest/retry/fn.never.html)
disables protocol-level retries too. A correlation header is not a promise of
provider idempotency. HTTP statuses are evidence; raw transport can capture a
429 response, while proxy normalization requires a 2xx status.

Private evidence includes prompts, input data, paths, hashes and service IDs.
Keep it in project-owned private storage. Stdout contains bounded status/flags or
stable error codes, without prompts, bodies, paths or credentials. No provider
account, project media or real voice is used in tests. Transport qualification is
not real-service qualification, semantic review, approval, selection, production
adoption or episode-scale readiness. Built-in service and speech adapters and
automatic native provider orchestration remain subsequent W3b work.

```text
cargo fmt --manifest-path crates/reel-workflow-http/Cargo.toml -- --check
cargo clippy --manifest-path crates/reel-workflow-http/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path crates/reel-workflow-http/Cargo.toml --all-targets --locked
```

CI runs portable transport/proxy tests on Linux and Windows, plus actual owned CLI
fixtures on Windows. Loopback tests cover exact bytes, private credential handling,
refusal before calls, redirects/statuses, deadlines, truncation, duplicate posts,
concurrent posts, lost metadata, replaced workspaces and preserved capacity.
