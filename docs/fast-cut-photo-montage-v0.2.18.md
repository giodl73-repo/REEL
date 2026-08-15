# Fast-cut photo montage v0.2.18

REEL CLI v0.2.18 makes rapid photo montage an explicit, opt-in edit mode. It
does not change `reel.manifest.v0.2` or the cinematic defaults used by existing
consumers such as BERTICA.

## Edit modes

- `--edit-mode cinematic` is the default. It retains the configured transition
  duration and established dissolve assembly.
- `--edit-mode montage` assembles shots with true hard cuts. Internally, FFmpeg
  concatenation replaces the invalid idea of a zero-duration crossfade. This
  mode sets the effective transition to zero, regardless of the cinematic
  `--transition-seconds` setting.

Hard cuts work well when pose, scale, uniform, headline, or location changes
supply the transition. Each beat remains a named manifest shot, preserving
timing and source lineage.

The artifact report records `edit_assembly` and the effective
`transition_seconds`, making the selected assembly behavior auditable without
reconstructing intent from the FFmpeg command.

## Punch treatments

- `motion: punch-in` moves from the full delivery crop to a centered 1.20 crop.
- `motion: punch-out` reverses that movement.

Both treatments use the selected curve in smooth mode and have deterministic
legacy equivalents. Crop-safety preflight samples the full 20 percent range,
so edge focal points and protected text regions can block a bad render.

## Example

```yaml
shots:
  - id: player-wide
    scene_id: player-burst
    start_seconds: 0.0
    duration_seconds: 0.7
    visual_asset: player-action.jpg
    motion: punch-in
    focal_point: { x: 0.5, y: 0.42 }
  - id: player-tight
    scene_id: player-burst
    start_seconds: 0.7
    duration_seconds: 0.55
    visual_asset: player-portrait.jpg
    motion: punch-out
    focal_point: { x: 0.5, y: 0.38 }
```

```powershell
reel animatic-render montage.yaml --asset-root assets --audio score.wav `
  --captions captions.srt --edit-mode montage --output review.mp4
```

The still-animatic renderer continues to accept one still `visual_asset` per
shot. Video trims, source-audio mixing, and multi-source layouts remain explicit
compositor work rather than being silently implied by this mode.
