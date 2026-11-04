# Exact browser PCM windows

The native browser source can now carry an explicit original-audio sample
window and integer linear fades. Previously a retained browser composition with
these controls rendered the correct excerpt length but omitted its envelope.
The implementation ports the integer sample arithmetic from the existing Rust
browser master producer into the bounded native runtime.

Producer-applied [.roles](../../.roles/ROLE.md) findings:

- Sound Designer: preserve the original source, exact sample endpoints and signed
  truncation. Standard floating-point fades are a different treatment. The exact
  private enrolled PCM comparison passes; no listening approval is asserted.
- Editor: match the excerpt to the declared frame duration, subtract its start
  before rounding shared anchors, and refuse out-of-window timing. No duration,
  picture selection or cue edit is authorized by this repair.
- Rights and Provenance Steward: retain held original primaries; normalization
  and enveloping create only owned transient intermediates. Producer evidence
  pins the implementation. Private fixture subjects stay outside Git. Technical
  correspondence does not advance selection, consent, delivery or release.

Validation: forty browser tests pass with explicitly enrolled local Edge and
FFmpeg, including the legacy browser fixture, native ordinary audio, measured
clock, exact integer-envelope mux, and corrupt-source cleanup. The envelope mux
test checks scratch retirement. Pure tests cover signed extrema, zero endpoints,
nonzero excerpt offsets, overlap, truncation, excess bytes and cancellation.
An explicitly enrolled private original also matches both retained selected and
enveloped PCM identities. This report is not an independent review.

Actual project picture/PCM correspondence, complete programme acceptance,
hosted qualification and lane activation/rollback remain separate open gates.
