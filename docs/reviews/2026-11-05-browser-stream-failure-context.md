# Native browser stream failure diagnosis

Windows CI run `42042431852` failed the SVG/exact-audio fixture with socket reset
10054. On the rebased repair-core head, run `42053863252` failed the exact PCM
window fixture with the same reset while the other three browser fixtures passed.
The browser implementation is unchanged from the source-inspector parent whose
hosted checks passed. These observations do not prove a browser crash, encoder
half-close race or infrastructure fault. Another unchanged retry is insufficient.

The existing outer `stream browser frames into native encoder` context spans
both the CDP control connection and the encoder input socket. Preserve the error
chain and add bounded stage context: command/direction/ID, capture frame, consumer
write/drain, encoder socket/half-close/child wait. Never include command parameters,
source URLs, resource bodies, PNG/base64 data or submitted text in this context.
Do not change acceptance, deadlines, resource limits, cleanup or retry behavior.

Injected control resets cover initialization and picture capture in both send
and receive directions. They must remain errors with the original I/O kind,
exactly one attempt, zero completed frames and no consumer completion. Consumer
write and drain failures must remain distinguishable. Existing real browser
fixtures continue to require exact picture/audio/clocks and owned cleanup.

Producer-applied review uses the editor and platform/audience lenses: frame and
stage identity make a failed production diagnosable without admitting partial
output, while payload-free context preserves the private source boundary. No
independent review, creative approval, runtime adoption or repaired-song delivery
is inferred. The underlying reset remains unresolved until a stage-bearing
failure or stronger reproduction establishes its cause.

Local validation: 24 capture tests and all four explicitly enrolled real
headless browser fixtures pass. The all-feature library suite passes 368 tests
with 15 explicit ignores; all-target/all-feature clippy, formatting and diff
checks pass. The local fixtures did not reproduce the hosted reset. Hosted
qualification remains required before integration.
