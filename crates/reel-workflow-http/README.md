# REEL workflow HTTP transport

An isolated Rust library/CLI for explicit provider or API-proxy JSON POSTs.
It performs real HTTP calls, but owns no Tig admission, candidate selection or
retained completion. Its dependency graph/lockfile is separate from the existing
REEL producer and native lifecycle pins.

```text
reel-workflow-http post CONFIG OPERATION_ID JSON_BODY NEW_INTENT NEW_METADATA NEW_RESPONSE --apply
```

```json
{
  "schema": "reel.workflow-http.v1",
  "endpoint": "https://your-authorized-provider.example/api",
  "timeout_seconds": 300,
  "request_byte_limit": 1048576,
  "response_byte_limit": 33554432,
  "secret_headers": {
    "authorization": { "variable": "PROVIDER_API_KEY", "prefix": "Bearer " }
  },
  "allow_loopback_http": false
}
```

The body is exact caller-authored JSON. Credentials resolve only from explicitly
named environment variables; errors and intent/response metadata omit their
values. HTTPS certificate verification remains enabled. Optional HTTP is limited
to literal loopback IP endpoints. URL credentials/fragments, reserved transport
headers, ambiguous case-duplicate headers and invalid header values refuse before
the call. The endpoint itself may be an API proxy. System/environment forwarding
proxies are disabled, as are redirects, decompression, HTTP/2 and automatic
retries. These choices use the pinned [reqwest client controls](https://docs.rs/reqwest/0.13.5/reqwest/blocking/struct.ClientBuilder.html).

`Config::prepare` validates/configures the request without sending it.
`Prepared::send_captured` invokes a required durable-intent callback before
network execution and provides received chunks to a private evidence sink.
`send` is the in-memory convenience variant; callers needing partial-response
evidence use `send_captured`. Request/response bounds are each 1–128 MiB; deadlines
are 1–21,600 seconds. Client retries are explicitly disabled rather than relying
on the library default. `x-reel-operation-id` correlates the original request;
it does not promise service-side idempotency.

The CLI exclusively creates private outputs, flushes its intent before sending,
flushes received prefixes, verifies successful response readback and writes
separate status/hash metadata. Existing intent always refuses another call,
including when new response paths are supplied. Timeout, disconnection, oversized
or truncated response and response-storage failure require reconciliation of
that original operation. Partial response files survive; no automatic cleanup or
fresh operation identity follows. The CLI expects caller-owned private output
directories; the native adapter must supply its guarded workspace/admission.

HTTP statuses are returned as evidence, including redirects, rate limits and
server errors. A bounded response is not provider success, a valid creative
artifact, a retained task or an approval. The
[timed ElevenLabs adapter](../../docs/handoff/2026-11-19-elevenlabs-adapter.md)
adds frozen source/model/scope checks and candidate WAV/alignment/timing outputs.
`send_observed` offers a pre-body header callback for request/history identifiers.
Real-account qualification and the built-in adapter's full native fixture remain
open. No real account, private media or voice was used in the loopback tests.

```text
cargo test --manifest-path crates/reel-workflow-http/Cargo.toml --locked --offline
cargo clippy --manifest-path crates/reel-workflow-http/Cargo.toml --all-targets --locked --offline -- -D warnings
```

Producer-applied privacy/provenance and recovery checks preserve the distinction
between request execution and retained/creative completion. They are not an
independent review or publication authorization.
