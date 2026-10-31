# Resume recorded retention without inventing a new outcome

An interrupted result adoption can leave a source at its prepared revision even
though its cached artifacts and immutable metadata were already published. The
new Rust recovery API verifies that exact packet under the original source fence
and completes the original operation. It requires a recorded prepare receipt;
it cannot create one to make an unrelated operation appear resumable.

Readback checks the admitted input root, job/source/owner/journal identities,
original revision, metadata tree roles, request bytes and limits, bounded
compressed evidence, declared output names and exact cached pins. Active-cache
read guards remain live through adoption. Verification budgets cover object
count, total bytes and per-read deadline. Success and failure packets use their
original distinct outcomes. Neither path creates a producer or approval claim.

The native runner fixture uses actual published success and failure packets and
models interruption after their prepare transition. It checks missing-prepare,
foreign owner/operation/job, stale revision, inadequate byte budget, corrupt
request, idempotent replay and rejection after newer work starts. Failed checks
preserve the exact pending reference and journal checkpoint. Recovery preserves
the original input predecessor and does not remove scratch.

Producer-applied provenance review: private locators remain outside project Git;
source identity and outcome remain explicit; no generic byte publication is
presented as native production. This is a technical checklist, not independent
approval. Missing-prepare recovery, verified workspace retirement, CLI wiring and
full consumer cutover remain open.
