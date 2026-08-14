# Pulse 05 — Scoped cadence evidence

Implemented REEL v0.2.16 after BERTICA voice experiments showed that emotion,
speaker register and terminal intonation are independent production facts.

## Delivered

- additive optional scope/register/contour/boundary/semitone/join fields in
  `reel.voice-performance.v0.1`;
- explicit `indextts25` engine selection without claiming unsupported native
  contour execution;
- validation for contradictory contours, terminal targets and join pauses;
- `voice-prosody-evidence` and `voice-prosody-evidence-check` commands;
- strict plan-receipt, measurement and rendered-audio hash binding;
- exact ordered span coverage and nonoverlapping measurement time bounds;
- three-part F0 and relative-semitone evidence with a 25-percent voiced-frame
  and 200-ms minimum reliability floor;
- path-free output, visible requested-versus-observed failures and mandatory
  human listening;
- sanitized pass and accidental-rise fixtures with no consumer text or voice.

## Boundary

REEL does not perform pitch extraction. A named/versioned analyzer supplies the
measurements. REEL validates, binds, evaluates and re-verifies the evidence. The
result does not establish emotion, identity, culture, age, gender or approval.
