# CI browser fixture startup budget

**Status:** prepared follow-up for CI PR #369; not yet qualified or merged.
The active M01 run/branch is unchanged.

Run `44828074724`, Windows media job `136605950868`, failed the corrupt-source
fixture at browser discovery, before reaching its intended resource-integrity
refusal. The log reports `browser discovery deadline elapsed`; three other native
browser fixtures passed. The command already used one test thread. M01's later
run `44863607774` passed the unchanged browser command, confirming the failure is
not consistently reproduced by that source or command.

The native browser's discovery/handshake uses the fixture's `max_io_wait_ms`.
Raise that test-only value from 15,000 to 30,000 ms to allow bounded cold hosted
startup. Keep the 60,000 ms overall capture limit, process/memory/workspace limits,
serial invocation and every integrity/cleanup assertion unchanged. This is a
revised CI fixture profile, not proof of a universal startup SLA or a change to
production runtime defaults. A corruption acceptance, wrong failure reason,
foreign-file deletion or overall timeout must still fail.

Rebase PR #369 on merged M01, then run the four actual native browser fixtures
with explicit enrolled Edge/FFmpeg/FFprobe and one test thread. Re-run the workflow
structure/shard tests and required hosted qualification. The prior split run's
longest successful lane was about 22 minutes, but the 30-minute target remains
unqualified until a complete fresh run passes. No check may be removed to meet it.

This record is one assistant's technical diagnosis, not an independent review or
production approval. Browser/media output remains controlled synthetic fixture
data; no private source, provider, asset cache or delivery target is involved.
