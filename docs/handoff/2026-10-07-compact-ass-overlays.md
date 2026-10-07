# Compact ASS scene overlays

Selected `h264-lossless` scene jobs previously encoded the clean picture as
H.264, then replaced it with FFV1 when applying an editable ASS title or poem.
The single ASS stage now honors the selected encoding, including lossless CRF0
and yuv444p. The delivery checker expects that same codec. The default remains
FFV1; timed video layers and post-camera output retain their existing behavior.

## Verification

- Before the fix, the new real-media assertion failed: `picture.mkv` was FFV1
  instead of H.264. Test command exited1.
- `cargo test --test scene_delivery`:18 passed,4 existing explicitly ignored,
  exit0. The ASS regression checks clean-picture/picture/master H.264 codecs,
  every decoded frame against default FFV1, byte-identical D/M/E/mix audio,
  identical native sample/frame clocks, and rejection of tampered ASS bytes.
- Existing timed overlays, picture transforms and post-camera tests passed.
- `git diff --check` passed.
- Read-only editor and rights/provenance checklist review by
  `/root/early_poem_join_delivery_check` found no blocker. Requested
  gpt-6-sol/high; actual runtime model/effort unexposed. The reviewer inspected
  source/diff and did not rerun tests or inspect media. No human approval or
  publication authority is inferred.

Consumer adoption must pin the new executable/revision and rerender affected
jobs. Previously completed FFV1 artifacts are not retroactively changed.
