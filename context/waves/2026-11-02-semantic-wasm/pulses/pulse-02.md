# Pulse 2: native adapter and working browser prototype

51 unchanged native assembly tests pass, with strict lint and wasm32 check.
Four standalone adapter tests and strict host/WASM lint pass. All 24 shared
production dependency versions, sources and checksums remain pinned unchanged.
The actual bounded release WASM is 383,670 bytes in the retained local build.

The first 16 real browser checks pass, including 36 exact native/WASM request
responses and seven independent controls through the unmodified production CLI.
They cover node/presentation/pointer behavior, native omissions and cycle scope,
computed-versus-declared locks, inert identifiers, original BOM/CRLF/LF copies,
worker deadlines/recovery, stale read/job fencing, keyboard/mobile use and actual
64 MiB memory-growth refusal. Native Rust/RUNE assumptions are not copied from
other repos. Source/hosted/default/live delivery is not yet established.

The expanded suite adds admitted graph/reference/byte controls, local run-request
exports and timing evidence. Full source/legal packaging, outside-Git source
replay and hosted/native production CI remain later gates. Original CLI binaries,
requests, logs and images stay private and outside the static served package.
