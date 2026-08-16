# Mixed-media timeline v0.2.20

REEL CLI v0.2.20 makes the production manifest the owner of mixed-media edit
events and stem assembly. A project no longer needs a custom script to decide
where stills, video clips, ambience, effects, or narration enter the timeline.
Existing manifests and the `--audio` pre-mixed-master workflow remain valid.

## Visual events

Ordered `shots` are visual events. Their conformed `start_seconds` and
`duration_seconds` still define the gap-free output timeline. Two additive
fields select how the visual source is read:

```yaml
shots:
  - id: opening-poster
    scene_id: opening
    start_seconds: 0.0
    duration_seconds: 1.0
    visual_asset: stills/opening.png
    media_kind: still       # default; REEL applies the declared motion
    motion: slam-in
    beat_marker_id: downbeat

  - id: goal-clip
    scene_id: opening
    start_seconds: 1.0
    duration_seconds: 0.8
    visual_asset: clips/goal.mp4
    media_kind: video
    source_in_seconds: 12.4 # trim point in the source clip
```

Video is scaled and center-cropped to the delivery canvas, converted to the
delivery frame rate, trimmed to the shot duration, and assembled through the
same hard-cut or crossfade timeline as stills. Source paths may be absolute or
relative to `--asset-root`; relative paths may not escape that root.

## Beat markers

`beat_markers` name exact points on the conformed timeline. A shot or audio
event can declare `beat_marker_id`; validation then requires its start to match
that marker within one millisecond. Markers may also carry a human label and an
`accent` flag for downstream planning.

```yaml
beat_markers:
  - { id: downbeat, time_seconds: 0.0, label: Opening hit, accent: true }
  - { id: goal-hit, time_seconds: 8.5, label: Goal impact, accent: true }
```

Markers outside the production runtime, duplicate ids, unknown references, and
off-beat event starts fail `reel validate` before rendering.

## Audio events

`audio_events` are independently timed sources with one of four roles:
`music`, `ambience`, `effect`, or `narration`.

```yaml
audio_events:
  - id: garden-room
    role: ambience
    source: audio/garden-room.wav
    start_seconds: 0.0
    duration_seconds: 18.0
    loop_source: true
    gain_db: -15
    fade_in_ms: 400
    fade_out_ms: 500

  - id: logo-hit
    role: effect
    source: audio/logo-hit.wav
    start_seconds: 8.5
    duration_seconds: 0.7
    beat_marker_id: goal-hit

  - id: welcome
    role: narration
    source: audio/welcome.wav
    source_in_seconds: 0.2
    start_seconds: 2.0
    duration_seconds: 3.4
```

An omitted event `duration_seconds` runs from its start to the end of the
production. REEL trims each source, applies gain and fades, places it at the
declared timeline offset, mixes the events, and pads/trims the final bus to the
manifest duration. `loop_source` asks FFmpeg to repeat a short source before
that trim.

## Narration ducking

When both narration and background events exist, `narration_ducking` routes the
mixed background bus through FFmpeg's sidechain compressor using the narration
bus as the detector. Narration is then mixed back into the ducked background.

```yaml
narration_ducking:
  threshold: 0.03
  ratio: 8
  attack_ms: 20
  release_ms: 300
```

Threshold must be in `0..1`, ratio in `1..20`, attack in `1..2000ms`, and
release in `1..10000ms`. If the policy is omitted, narration and background are
mixed without ducking.

## Final mastering

An optional `audio_mastering` policy applies one manifest-owned final loudness
and peak stage after event mixing and before the exact runtime trim:

```yaml
audio_mastering:
  integrated_lufs: -17.7
  loudness_range_lu: 11
  true_peak_dbfs: -2
  limiter: 0.75
```

REEL uses FFmpeg `loudnorm` followed by a limiter with automatic makeup gain
disabled. The values and the presence of mastering are bound into artifact
lineage. Mastering is available only with manifest `audio_events`.

## Rendering and evidence

Manifest audio events are an audio mode, so do not also pass `--audio` or
`--silent`:

```powershell
reel validate production.yaml --output json
reel animatic-render production.yaml --asset-root assets `
  --captions captions.srt --edit-mode montage --output review.mp4
```

Use `--no-captions` instead of `--captions` for a deliberately clean visual
cut. `--encoding-preset medium|slow` controls H.264 encoding speed while `slow`
remains the compatibility default. REEL bounds each looped-still decoder to one
thread, normalizes every event to square pixels, and transparently executes
large graphs through a temporary filter script; the artifact report still
retains the complete deterministic command graph.

The artifact report hashes every still, video, and audio-event input. Its
`mixed_media` lineage records still, video, audio-event, and beat-marker counts
plus whether narration ducking was active. `animatic-check` reconstructs those
counts from the bound manifest and rejects stale or inconsistent evidence.
