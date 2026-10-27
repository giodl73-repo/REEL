# Shutdown handoff: native Journey caption runtime and selected MP3 support

## Explicitly resumed completion, 2026-10-26

User-authorized all-four-Journeys continuation resumed this bounded REEL lane.
MP3 source/test commit: `4c9ac37fac680f66d4f0a17fee8fc1e566688b95`.
Production delta is the selected-source-only stereo MP3 allowlist entry. Native
PCM24 policy remains strict. The new integration regression removes the adoption
manifest and requires rejection before any output, at both 25 and 30 fps.

Completed checks at that source:

- `cargo test --locked --test presentation_adopt --target-dir
  C:/src/REEL-worktrees/journeys-native-captions-target`: 18 passed, including the
  retained H264/MP3 complete decoded identity, normalized PCM prefix, repeat-only
  clocks, silent holds and missing-adoption rejection at both delivery clocks.
- `cargo test --locked --lib episode_conform --target-dir
  C:/src/REEL-worktrees/journeys-native-captions-target`: 15 passed.
- `cargo clippy --locked --all-targets --all-features --target-dir
  C:/src/REEL-worktrees/journeys-native-captions-target -- -D warnings`: exit 0,
  rechecked after final regression assertion.
- Scoped `rustfmt --edition 2024 --check src/episode_conform.rs
  tests/presentation_adopt.rs` and `git diff --check`: exit 0.
- Full `cargo fmt --all -- --check` finds pre-existing unrelated formatting in
  `src/browser_composition.rs` and `src/scene_delivery_compile.rs`; left untouched.
- This REEL worktree has no `tools/validate.py` or TRACKER ROLES structural
  checker. CAMINOS registry/checkpoint gates remain with its owning lane.

Both requested release builds completed with `--locked` and the isolated target
directory, then were checked again after the source commit. Release directory:
`C:/src/REEL-worktrees/journeys-native-captions-target/release`.

| Executable | Bytes | SHA-256 |
| --- | ---: | --- |
| `reel-presentation-adopt.exe` | 912384 | `eefdece9498e844235ee112dfd5b8adb20ad02b0f2825fcdf23bbd7e58520a26` |
| `reel-episode-conform.exe` | 6402048 | `9075c20d995375db8e4368d3ee8deed2da63239da4b723b4cbe5cdb94d8f87da` |

Existing caption release was independently rehashed: same SHA and 1233408 bytes
as below. No selected media hashes, original source bytes, CAMINOS registries,
shared executables, cache objects or retained temp fixtures were changed.

Producer-applied review used the editor, sound designer and rights/provenance
checklists: complete ordered frame/sample assertions preserve retained content;
normalization and explicit holds preserve PCM prefix and zero-only hold audio;
missing adoption evidence rejects. Technical validation grants no human approval.

Additional broad integration run completed exit 0: `cargo test --locked --test
presentation_adopt --test episode_conform --test episode_conform_ordered_units
--target-dir C:/src/REEL-worktrees/journeys-native-captions-target`: conform 1/1
passed (907.96 seconds), ordered units 9/9 passed (600.21 seconds), presentation
adoption 18/18 passed (19.16 seconds). All owned test/build sessions are terminal.

## Historical shutdown snapshot (superseded by completion above)

Owned lane: `C:/src/REEL-worktrees/journeys-native-captions`, branch
`feat/journeys-native-captions`. Current committed HEAD:
`e84d4056ce96eccb21b06ba9fc187f7d1a276c10`, preceded by
`ad2d09ae7f135ff0adce3ba8f2aec9a291f081c7`; base `04a231c`.

Shutdown requested before completing the subsequent MP3 capability patch. Do not
start jobs/builds/tests or media execution automatically on resume. No CAMINOS
files, cache objects, shared executables, or prior runtime pins were changed.

## Completed caption runtime, preserved

`C:/src/REEL-worktrees/journeys-native-captions-target/release/reel-episode-captions.exe`
SHA-256 `59dde7ac3110e69db7e35d0def91418ab1fb4efadfb9b1c0406f3c28afaf1523`,
1233408 bytes. Source contract remains committed e84d405; this release executable
was not rebuilt or overwritten during the MP3 work.

## Uncommitted MP3 patch

- Modified `src/episode_conform.rs`: one production allowlist addition, `mp3`,
  within `allow_encoded_source` only. Strict native lossless policy is unchanged.
  Adds a source-format guard unit test: selected stereo MP3 accepted and requires
  normalization; native MP3, mono MP3, stereo Opus and PCM float remain rejected.
- Modified `tests/presentation_adopt.rs`: optional encoded-audio hook for existing
  adoption fixture; new H264/MP3 retention test at 24→25 and 24→30. Checks exact
  selected byte identity, independent full decoded picture/audio hashes/counts,
  complete normalized PCM prefix, native segment sample/frame accounting and
  two-second zero-audio holds.
- Diff before this handoff: 2 files, 190 insertions and 7 deletions. No MP3 source
  commit, Clippy result, or fresh release adopter/conform pin exists yet.
- An attempted addition of an explicit unverified-MP3 conform regression failed
  patch-context matching before any edit. Subsequent commands in that invocation
  were not dispatched. That regression remains to be added.

## Checks actually completed

- `cargo test --locked --test presentation_adopt retained_h264_mp3 --target-dir
  C:/src/REEL-worktrees/journeys-native-captions-target -- --nocapture`: 1 passed,
  including both output clocks and independent full decode/PCM/hold assertions.
- `cargo test --locked --lib episode_conform --target-dir
  C:/src/REEL-worktrees/journeys-native-captions-target`: 15 passed, including
  MP3/stereo/unsupported-codec/native-policy rejection checks. Exit 0.
- Changed files were formatted with rustfmt. Full presentation-adopt suite and
  Clippy for this MP3 patch have NOT run. Both required release binaries have NOT
  been rebuilt for the MP3 patch. Own debug executables were built by tests.

## Actual selected CVA movie fixture

Exact named source was read SHA/byte-verified from existing E cache, without
storage CLI or cache mutation. The original source was not transcoded/swapped.
Original producer v13 phase2 adoption-CVA-es-v3.json and its five exact FileRefs
were staged to:
`C:/Users/giodl/AppData/Local/Temp/reel-retained-mp3-CVA-bb185ee50f234cd8bad922ae16c0dfdb`.

Own current debug adopter completed `build adoption.json` against that root and
`adopted/receipt.json` is present. Native retain-source result:

- selected source AND retained master SHA-256:
  `09f3e218d341ae6107fc8f82f4be674868e2a575f286dd8f3a7ca0a9728efdf4`
- source/master: 27321269 bytes; 2880 frames; 5760000 samples.
- decoded picture SHA-256:
  `6c7428b454709451cc47f18948583586995b39114fd2b124a4ee5af395d377df`
- decoded audio SHA-256:
  `88a4a35043206d6596078e7be296358c2c8b1fef70d557731da71eccf959503c`
- timestamps_verified and source_content_matches_lossless_master are true;
  technical_validation_state is decoded-source-equivalent.

This proves actual source retention under the debug patch, not final source-pinned
release adoption or the new CVA intro+film conform integration. Human approval is
not inferred. Keep these temporary inputs/outputs in place; no cleanup attempted.

## Process state and next action

All owned command sessions are terminal. Session 27029 (actual CVA adoption) and
19673 (native conform unit tests) both completed exit 0 after shutdown steering.
No owned build/test/render subprocess remains active; agent can stop.

Next single action when explicitly resumed: add the missing regression that
removing the independently rechecked adoption manifest cannot admit MP3 through
ordinary native episode input. Then finish relevant tests/Clippy, source commit,
and isolated release builds of reel-presentation-adopt and reel-episode-conform.
Preserve current caption executable/pin and previous shared binaries.

## Earlier blocked temporary cleanup

Automatic approval review rejected removal of
`C:/Users/giodl/AppData/Local/Temp/reel-caption-fixtures-5ddf014df3244ea0b0fe543c73c22c41`
with reason exactly `blocked by policy`. No retry or approval request was made;
that directory remains untouched. Leave it in place on resume.
