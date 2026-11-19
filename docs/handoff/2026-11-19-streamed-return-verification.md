# Bounded streamed return verification

`reel_workflow::check_output_reader` verifies an exact named candidate from a
`Read` source using a fixed 64 KiB buffer. It checks the packet/result contract
and caller allowance before reading, then checks total length, SHA256 and EOF.
Short reads and ordinary interrupted reads are supported; truncated, trailing or
corrupt bytes refuse. Other I/O failures return structured `NeedsRecovery` and
do not establish an output result, retention, approval or a retry permission.

The native response adapter now uses this path for outputs requiring only byte
verification. It holds the same ordinary-file and ancestor guards through native
ingestion, rather than allocating a `Vec` for each whole output. The shared
`staging::open_held` retains the previous Windows no-write/no-delete sharing and
reparse/ordinary-file checks. Source/Index/protection/publication/retained-owner
operations are unchanged.

PNG candidates still use the bounded complete decoder. Structured-review report
bytes still pass the bound report/context/evidence validator. Deterministic tool
returns still receive their existing independent producer verification. A
streaming hash cannot replace those semantic checks or authorize returned code.

This removes a concrete buffering barrier in the existing return path. It does
**not** qualify the [S5 episode-scale consumer](2026-11-19-s5-consumer-scale.md):
the existing 128 MiB return bound and v1 movie/admission/staging limits remain in
force. Explicit large-output budgets, streamed producer capture and historical
media verification, bounded scene/chapter composition and qualified managed
protection/recovery are still required together. No limit was silently raised.

Three contract tests exercise short-read/interruption behavior and maximum read
buffer size, exact bytes/EOF and I/O refusal, and contract/allowance refusal before
any read. All 102 workflow tests, 63 standard native tests (including the 247 SDK
source-pin checks), workflow Clippy and strict all-target native Clippy with and
without film support pass. This is component/standard-suite evidence; the
separate running automatic-resolution fixture
uses its previously frozen source/runtime and does not qualify this new adapter.
