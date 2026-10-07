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

## Local-anchor correction after actual E1 intake

Actual E1 ES/EN intake rejected global source continuity: an AAC clock reset
at the 120-second opening boundary precedes the selected scene. Decoded frame
5626 has PTS5760001 at 48kHz versus cumulative5761024 samples; the next frame
has PTS5761024 versus cumulative5762048. This diagnosis does not authorize a
manual sample offset or replacement mix.

The importer now finds a unique half-open decoded audio frame interval at the
selected picture PTS, records its native cumulative sample ordinal and PTS,
and validates audio continuity only across the selected excerpt. Earlier audio
resets are allowed. Rendering/source verification share the measured window;
bounded reads include its cumulative end sample. Original first audio PTS is
retained separately. The frame-index picture continuity guard remains intact.

Synthetic local-reset fixture independently slices whole-source decoded PCM
at samples29008..77008 and verifies exact adopted content. Additional fixtures
reject overlapping coverage at the selected anchor and a reset inside the
selected range. Original11-fixture run passed; expanded13-fixture result and
final independent source review are recorded at the producer checkpoint.

Final producer validation after the full-metadata probe change:
`cargo test --locked --test presentation_adopt -- --test-threads=2` passed all13.
The positive oracle now asserts anchor tick576 and cumulative ordinal27648
without the reset, 28672 with it. The complete metadata probe avoids a fixed
pre-range reset limit; actual FFmpeg media reads remain bounded. Final actual
read-only reviewer found no static correctness blocker, and flagged full frame
metadata memory use for long sources. It did not rerun media or tests. Reviewed
source SHA a14ab71a532159bf449bec37d019eecc0bef216d5f23ebba6fca65e23b2a401d,
tests SHA 2bf6010ff97976c854fa8ca69e41bf67b4fbb9f86972e06fa1086c4946dbeb96.
