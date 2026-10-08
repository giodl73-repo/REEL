# Explicit episode delivery geometry review

## Scope

Add optional output geometry to the generic episode conform manifest. Selected
source media and source receipts retain their original identities. Legacy
manifests keep their existing shape and reject mixed dimensions. Explicit
scaling preserves aspect ratio; no crop, stretch, source replacement or retiming.

## Executed evidence

- Initial `cargo test --test episode_conform_ordered_units`: terminal zero,
  five tests passed, including all four existing ordered-display, source-text,
  compact-clock and verification-cache fixtures. This preceded the final
  explicit-geometry timestamp and color checks.
- Final `cargo test --test episode_conform_ordered_units
  mixed_geometry_requires_explicit_aspect_preserving_delivery_policy -- --exact`:
  terminal zero, one test passed in 28.15 seconds. Both languages exercise
  mixed32/64 square-pixel sources, H264 lossless intermediates, compact output,
  exact external transformed-source digest, unchanged frames/audio samples,
  original source hash, and no-policy/aspect-change/unsupported-policy/SAR2:1
  rejection. The final source changes apply only to explicit output geometry.
- Final `cargo clippy --lib --test episode_conform_ordered_units -- -D warnings`:
  terminal zero. `cargo fmt --all`, `git diff --check`, and role-file validator
  passed. Windows link attempts while the previous test binary was live failed;
  these are not test passes. No live test was restarted.
- Compact fixture initially caught loss of the known limited-range color tag
  in MP4. Encoder flags alone did not preserve the default H264 VUI signal;
  explicit H264 range bitstream metadata now retains it. The strict compact
  metadata check remains enabled.

## Review lenses and independent evidence

Editor: original source timestamps, frame counts, audio counts and ordered
decoded output checks remain mandatory. Scaling does not alter cut points.

Rights/provenance: original selected FileRefs and receipts are rechecked.
Receipt additions record original size/SAR/color tags, original decoded picture
hash, output size and transformed-source equivalence. None establishes creative
approval or publication authorization.

Actual read-only specialist `dated_chapters_native_delivery_check`, requested
Sol/high, runtime identity unexposed, run01a118e5-a981-7140-b23e-bcce87c5536b,
found source PTS and output color/SAR gaps. Both were addressed. Its revised
static review found no concrete blocker (reviewed diff4fcec92aac95e7d9a542f0e34c0ae708c5dffa61e0428617df28029c88965d2c),
before the final compact range bitstream fix. It did not execute the tests or
grant human approval. Stream tags are checked; dynamic per-frame color side
data is outside this delivery policy's verification scope.

## Delivery limits

Explicit geometry requires square-pixel, equal-aspect sources and consistent
stream color tags. Unknown tags remain unknown. Upscaling adds no source detail.
Complete private-film review remains a consumer responsibility.
