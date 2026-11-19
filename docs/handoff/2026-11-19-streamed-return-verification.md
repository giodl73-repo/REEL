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

## Movie return and historical verification

`film_render::capture_output_pins` builds the same worker receipt from content
hash/length observations, without requiring whole-media byte buffers. Its closed
path set, exact bound task, output media types and declared byte budgets are
checked exactly as in `capture_outputs`. Pins are ordinary Rust data and confer
no file protection, execution proof or custody. The existing buffered API still
hashes its supplied bytes and produces byte-identical receipts.

The native movie return checker and retained historical proof now independently
stream each media file through held ordinary handles, then use those verified
identities for receipt correspondence. They buffer only the small execution
request/result controls and, when requested, review-ID metadata. The native
provider guards remain live through historical proof; return-file guards remain
live through independent primary/media checking and ingestion. The existing
contained media verifier still performs its checks; hash pins do not replace it.

A contract regression compares complete receipt bytes between the two capture
paths and refuses missing files, invalid hashes and overflowing byte demand.
All 103 workflow tests, all 63 standard native tests (including 247 SDK pins),
workflow Clippy and native strict all-target Clippy with/without film support
pass. The explicit read-only historical movie fixture is recorded after execution.
The latter uses the already retained synthetic movie after its source slot was
closed/reused, compares the full previously exported review copy/IDs and checks
source activation fencing and unchanged claims. It creates no production job.

This further removes whole-media buffering from **verification**, not producer
capture. `film_process::Rendered` / `movie::Generated` still carry buffered
producer outputs, and v1 movie, staging, return and admission limits remain.
Streamed producer capture plus explicit large-job budgets and bounded native
composition/recovery remain necessary for S5; this change does not adopt S5 or
modify existing E7 jobs/approvals. Editor and rights/provenance checks preserve
unchanged identities, clocks and technical/creative authority separation.

The historical fixture now passes in 102.36 test seconds using source
`10d5ea4043d32df6dad8ebe14e9a9f6bd0b267de`. Its 1,926 tracked files are archived
and individually hash/length verified; the frozen optimized inspection test is
`b2da757dc5db9dc417a0fc267f75fdbac45b3118fd0c93222ef2b432815a8ef1`
(14,686,208 bytes). Both ordinary historical proof and review metadata capture
reopened against the previously qualified media verifier. The complete exported
review copy and IDs match, native claims are unchanged, and the original source
activation remains refused after slot reuse. No production job, render or native
publication was created. Exact private evidence is
`F:/src/tig-dev-temp/reel-streamed-movie-20261119-v1/qualified-tools.json`.
