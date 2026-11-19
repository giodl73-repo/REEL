# Bounded provider/proxy HTTP calls

The new isolated `reel-workflow-http` crate supplies real blocking JSON POSTs,
including an explicit CLI. See its [contract and configuration](../../crates/reel-workflow-http/README.md).
Existing REEL producer/native locks and qualified executables remain separate.
The transport pins reqwest 0.13.5 and its complete lockfile. Request composition,
native admission/publication and provider-specific semantics remain outside this
transport; no installed pipeline or service/account integration is inferred.

Eight tests pass: exact request/response identity and credential headers; durable
intent before connection; preflight/credential refusal without network effects;
redirect suppression and complete rate-limit status evidence; oversized/truncated
body refusal; no second connection after a disconnected POST; timeout uncertainty;
and CLI intent/prefix preservation with refusal to reissue an original operation.
Strict Clippy passes for all targets. Tests use synthetic loopback TCP endpoints,
never a real provider account, private media or voice. The generic response does
not establish model identity, semantic quality, creative approval or retention.

The CLI's durable intent callback and incremental private byte capture prepare
for protected native integration. That adapter must supply owned workspace guards,
actual declared input proof, preserved original operation identity and response
normalization before native candidate publication/retained acknowledgement.
Uncertainty must reconcile the original call; writing an empty response file or
timing out cannot justify another provider request.

Provider-specific image/text/ElevenLabs adapters, raw response evidence retention,
service operation lookup, contextual review/fixes and the full project completion
ledger remain open. Producer-applied privacy/provenance and recovery checklists
are not independent review, speaker consent or release authority.
