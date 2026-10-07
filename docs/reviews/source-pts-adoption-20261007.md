# Decoded source clock adoption review

Scope: optional evidenced `audio_range_origin` for generic frame-exact
presentation excerpts. This does not change the default sample-index policy.

## Sound and editing lenses

The selected picture's decoded PTS and first decoded audio PTS determine the
sample window. Checked rational arithmetic computes the complete end expression
before rounding. Rendering and independent source verification share that window;
continuous output audio timestamps remain a separate evidenced policy. No
resampling, padding, performance rewrite or independently authored offset.
Source clocks through the range are validated; unavailable, negative, overflow
and discontinuous ranges fail. The input read is bounded by the selected clock.

## Rights and provenance lens

Policies require exact hash-bound selection evidence. Observed timestamps/time
bases and sample bounds are retained in the adoption receipt, rebuilt by `check`
and episode conform, and covered by the verification cache's manifest, source,
receipt, executable and media-tool keys. Technical equivalence grants neither
creative selection nor publication. Tests use generated synthetic media.

## Executed evidence

- `cargo test --locked --test presentation_adopt -- --test-threads=2`: 10 passed.
- `cargo test --locked --lib presentation_adopt::verification_cache_tests`:
  3 passed.
- The H.264/AAC fixture uses actual video PTS 583/1000 and audio PTS 0/1000,
  with native 48 kHz window 27984..75984. Its oracle decodes the complete source
  independently and slices raw PCM/frame bytes. It exercises full adoption,
  receipt recheck and episode conform.
- A repair-only build passes output timestamp checks but has a different PCM
  digest, proving that timestamp repair alone does not select the new window.
- Negative fixtures reject a picture gap before the selected range and an
  audio gap despite output timestamp repair. Evidence rejection, past-end,
  default paths, native-byte preservation and cache invalidation remain tested.
- Unit tests cover fractional sample carry, negative bounds and overflow.
- `git diff --check`: passed.

Actual bounded read-only reviewer: `/root/early_poem_join_delivery_check`,
thread `01a11545-7487-74e1-b781-f6919254043c`, requested
`reel_delivery_reviewer`, `gpt-6-sol/high`; actual model/effort unexposed.
The reviewer inspected source, tests, docs, conform routing and verification
cache logic, finding no static blocker. Tests above were executed by the producer;
the reviewer did not rerun them. These role lenses are engineering findings,
not real-person approval or a listening review of production media.
