# Selection locks and audio cache v0.2.21

REEL CLI v0.2.21 separates approval state from the manifest that produced a
proof and separates picture rendering from iterative audio work. Both workflows
are additive; normal `animatic-render` behavior and `reel.manifest.v0.2` remain
compatible.

## Lock a selected proof

Do not edit a rendered manifest merely to label its proof locked. That changes
the manifest hash and makes the proof's artifact evidence stale. Instead, pass
the verified artifact report to `animatic-lock`:

```powershell
reel animatic-lock review.artifacts.json --output-dir locks/selected-v1
reel animatic-lock-check locks/selected-v1
```

The command first runs the full animatic check. It then atomically writes:

- `manifest.locked.yaml`, a locked governance derivative;
- `selected-artifact.json`, the exact verified artifact report; and
- `selection-lock.json`, a receipt binding the source manifest, locked
  derivative, selected artifact report, and selected video hashes.

The selected video remains outside the packet. Its path and hash remain in the
private artifact report, and `animatic-lock-check` re-verifies it in place.
Creating a lock never rewrites the source manifest or selected proof.

## Begin a later revision

Locked manifests are immutable planning baselines. Create a new conformed
derivative before changing timing, edit, visuals, or sound:

```powershell
reel planning-derive locks/selected-v1/manifest.locked.yaml `
  --output production/next/manifest.yaml `
  --reason "alternate score pass" --changed-dimension mix
```

The derivative records its parent, reason, changed dimensions, timestamp, and
review-candidate state. Reel refuses an unlocked source, an empty reason or
dimension list, and any overwrite.

## Render audio without picture

For a manifest with `audio_events`, compile an AAC review mix without decoding
or encoding any visual source:

```powershell
reel animatic-audio-render production/next/manifest.yaml `
  --asset-root assets --output review/mix-b.m4a
reel animatic-audio-check review/mix-b.audio-artifacts.json
```

The audio preview uses the same event trims, gains, fades, offsets, role buses,
narration ducking, mastering, and exact timeline trim as the full mixed-media
renderer. It encodes a 192 kb/s AAC review master. Its private, path-bearing
report hashes the manifest, audio policy, every audio source, and the rendered
mix; use Reel's share-safe receipt workflows when evidence must leave the local
production boundary. `--dry-run` writes only the deterministic report.

## Reuse verified picture

When the picture edit is unchanged, combine a previous verified animatic with a
verified audio preview:

```powershell
reel animatic-remux review/picture.artifacts.json `
  review/mix-b.audio-artifacts.json --output review/picture-mix-b.mp4
reel animatic-remux-check review/picture-mix-b.remux-artifacts.json
```

Reel verifies both source artifacts, requires the same work id and matching
durations, copies the H.264 picture stream without re-encoding, replaces the
audio with the AAC preview, and writes a report binding every source and output
hash. This workflow is intentionally explicit: Reel never guesses that an old
picture should be reused.
