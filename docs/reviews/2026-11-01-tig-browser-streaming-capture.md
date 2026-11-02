# Bounded Tig browser capture interface

The legacy browser renderer copies primary files and retains a PNG for every
frame before encoding. The candidate native interface instead serves exact
guarded resources over CDP and passes each validated frame synchronously to a
consumer. It preserves the existing frame clock and background-mask meaning.
It creates no media copies, frame spool, output directory or browser profile.

Producer-applied Editor and Rights/Provenance checklists require unchanged
timing/geometry, explicit clean-picture behavior, exact input authority and no
external resource fallback. Identity/storage/recovery checks require bounded
aliases/requests/messages, no frame queue and no completion after protection or
consumer failure. Neither the interface nor this review grants human approval,
provider custody, output reuse or eviction authority.

Local fixtures cover paused-resource acknowledgement ordering, exact virtual
URLs and bytes, body/range/redirect/foreign-origin refusals, request/byte/event
budgets, source loss, decoder corruption and geometry, background masks, frame
clock errors, script exceptions, late failure and consumer final drain. Actual
loopback WebSocket fixtures test message bounds, command roundtrip and an
incomplete trickling frame against an absolute deadline. They do not execute
Chromium, an encoder or private media.

BROWSER-02 remains incomplete: the managed runtime must supply process/storage
admission, program protection, browser profile ownership, bounded native encoder
consumption and process-tree cleanup. BROWSER-03 managed output correspondence,
historical replay, private full-reference comparison, hosted qualification and
lane activation/rollback remain open. No active tool binding changes here.
