# Lossless encoding through scene compositors

Production found a 183-second camera scene produced a 6.7 GB picture even
though its delivery profile selected H264 lossless. Camera and overlay passes
overrode the selected setting with FFV1.

All initial, ASS, single/multiple timed-overlay and post-compose-camera passes
now share the selected lossless encoder. Default remains FFV1. H264 uses CRF0
and yuv444p. Filters, source selection, frame/sample clocks, audio and mix are
unchanged. Final checking expects the selected codec.

Actual validation: `cargo test --test scene_delivery -- --test-threads=2`:
18 passed, 4 pre-existing ignored FFmpeg integration tests. Four exercised
compositor fixtures compare every final decoded picture byte and PCM sample
against the alternate encoder, and verify final codec names. Their existing
semantic span and tamper checks remain active.

Independent read-only code review: `/root/efficient_reel_scene_outputs` inspected
the implementation and tests and returned scoped PASS. Editing lens: no cuts,
timing or composition changes. Sound lens: mix code untouched and decoded PCM
parity asserted. This is AI engineering review, no human creative approval.

Compatibility: preserve historical pinned executables for verifying old FFV1
camera/overlay outputs that previously ignored an H264 setting. Use new output
directories for new builds. Scene movies remain temporary intermediates; final
episode compact delivery and authorized cleanup retain their separate controls.
