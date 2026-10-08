# Declared color interpretation and square presentation canvas — 2026-10-08

Actual production preflight: old E5 photo captions carry known JPEG-derived
bt470bg matrix, while other inputs leave it unspecified. Portrait844x1249 was
fitted to487x720 by old still renderer and leaked compensating SAR0.999041533
into the1280x720 canvas. Original source photo pixels are square.

Generic conform adds optional assume_color_space_for_unspecified=bt470bg.
Known matrices never change; range, transfer and primaries still match strictly.
Original tags and explicit assumption remain in receipts. Independent RGB24
original/declaration hashes must match before resize; YUV transform conservation
is a separate check. This does not claim unknown primaries/transfer calibration.

Generic still renderer fits source displayed aspect to rounded integer dimensions
before a square-pixel padded canvas. Sources/pictures, caption ASS and clocks
remain explicit hash-bound inputs. No existing master is retagged.

Evidence:
- Initial test51780 PASS color fixture used insufficient YUV equality; retained
  as pre-fix evidence, never used to claim display-color safety.
- Independent bounded static review exposed that error with actual FFmpeg
  YUV/RGB repro. Corrected helper source6417b0cf... re-reviewed, no new concrete
  blocker. RequestedSol/high, actual runtime unexposed; no human approval.
- Corrected31334: actual RGB negative unit PASS0.42s (equal YUV but differing
  displayed colors rejected; same matrix accepted). ES/EN integration PASS39.79s.
- Actual21236 full still-sequence suite PASS5,4.26s: square, portrait, known
  non-square source display fit, audio envelope and semantic song clocks.
- Final clippy, format, diff and roles PASS. Full E5 conform still pending.
