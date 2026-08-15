# Trailer motion v0.2.19

REEL CLI v0.2.19 adds an opt-in motion vocabulary for sports, game-launch, and
animated-series montage. Existing manifests, the default `cinematic` assembly,
and the default `ease-in-out` curve remain unchanged.

## CapCut benchmark

The useful lesson from current CapCut is not its template library; it is the
combination of reusable camera-movement presets, property keyframes, speed
curves, motion blur, beat-aware timing, subject-aware reframing, and layered
sound effects. REEL already owns deterministic timing, manifests, motion
lineage, hard-cut montage, and audio verification. This release takes the next
small reusable step: auditable high-energy camera treatments.

Official references reviewed:

- <https://www.capcut.com/tools/ai-movement-tracking>
- <https://www.capcut.com/resource/how-to-add-keyframes-in-capcut>
- <https://www.capcut.com/resource/how-to-do-velocity-on-capcut>
- <https://www.capcut.com/tools/sound-effects>

## New treatments

- `motion: slam-in` rapidly closes from the delivery crop to a centered 1.28
  crop. It is suited to a goal, impact pose, or celebration freeze-frame.
- `motion: whip-right` traverses a 1.12 crop from left to right with a fast
  ease-out trajectory.
- `motion: whip-left` mirrors that traverse.

All three have smooth and deterministic legacy implementations. Crop-safety
preflight samples the full transform range, so an edge focal point or protected
region can block an unsafe render.

## New curve

`--motion-curve ease-out` applies a cubic ease-out to established motion
treatments. The two whip treatments and `slam-in` retain their intentionally
faster fourth-power impact trajectory.

```powershell
reel animatic-render trailer.yaml --asset-root assets --audio score.wav `
  --captions captions.srt --edit-mode montage --motion-curve ease-out `
  --output trailer-review.mp4
```

## Follow-on candidates

These remain explicit future work rather than implied behavior:

1. beat-marker input and snap-to-beat timing validation;
2. per-shot transform keyframes;
3. optical-flow speed ramps for video inputs;
4. subject-aware reframing and layered 2.5D parallax;
5. manifest-owned sound-effect events and ducking.

That order keeps authored timing and evidence ahead of convenience automation.
