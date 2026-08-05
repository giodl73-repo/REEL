# Vertical sound-off role proof

This sanitized `reel.manifest.v0.2` derivative proves a six-second, phone-first
9:16 output that remains complete without an audio stream. It uses only
synthetic captions and tiny text-based PPM frames.

```powershell
cargo run -- validate manifests/fixtures/vertical-sound-off/manifest.yaml --output json
cargo run -- source-coverage manifests/fixtures/vertical-sound-off/manifest.yaml --output json
cargo run -- quality-check manifests/fixtures/vertical-sound-off/manifest.yaml --output json
cargo run -- animatic-render manifests/fixtures/vertical-sound-off/manifest.yaml `
  --asset-root manifests/fixtures/vertical-sound-off `
  --silent `
  --captions manifests/fixtures/vertical-sound-off/captions.srt `
  --width 720 --height 1280 `
  --output target/vertical-sound-off.mp4
```

The role decision record is in
`docs/reviews/2026-08-04-vertical-sound-off-role-review.md`. Role findings do
not imply principal approval; the manifest deliberately retains
`principal_approved: false` and an empty `principal_findings` list.
