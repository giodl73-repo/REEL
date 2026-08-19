# Performer and sprite visibility windows — v0.2.30

REEL choreography previously assumed that every performer occupied the stage
for the entire shot. That is wrong for entrances, exits, substitutions, scene
crossings, off-screen handoffs, and any tracked event whose participant set
changes over time.

## Choreography contract

A performer may declare an inclusive beat window:

```yaml
performers:
  late-arrival:
    start: entrance-mark
    visible_between: [entrance, final-pose]
    phrases:
      - { action: approach, to: action-mark, between: [entrance, contact] }
```

Both beat IDs must exist and move forward in time. Approaches, handoffs, and
reactions must remain inside the performer's window. A handoff also requires
the target to remain visible for the complete transfer. The initial owner of a
prop must be visible at frame 0.

The resolved choreography plan records `visible_start_frame` and
`visible_end_frame` for every performer. The abstract preview omits a performer
outside that inclusive range; path overlays may still show the complete planned
route for review.

## Sprite execution

The production manifest supports the same inclusive range directly:

```yaml
sprites:
  - id: late-arrival
    visible_start_frame: 48
    visible_end_frame: 180
    keyframes: [...]
```

Both fields are required together. The start must not exceed the end, and the
end must remain inside the shot. REEL keeps the complete keyframe timeline but
adds the visibility range to every applicable FFmpeg overlay segment. This is a
hard editorial entrance or exit, not a fade and not a substitute for an
authored transition pose.

`choreography-sprite-manifest` carries performer visibility from the resolved
plan into generated sprite tracks. Props remain independently visible according
to their own tracks and handoff chain.

Asset bindings may lower `performer_path_subdivisions` (default `6`) and
`prop_path_subdivisions` (default `8`) for fast review proxies or an
intentionally pose-to-pose aesthetic. Values are bounded (`1..=12` for
performers and `1..=16` for props), so a binding cannot silently create an
unbounded render graph. These controls affect compiler sampling only; marks,
beats, handoffs, visibility, and production identity remain unchanged.

On Windows, external FFmpeg and ffprobe invocations are written to a temporary
WSL shell script and launched by its short path. This avoids the Windows process
command-line ceiling for sprite-heavy renders while preserving the same quoted
arguments and WSL working-directory behavior.

## Compatibility

The fields are optional. Manifests and choreography sidecars without them keep
the previous full-shot visibility behavior.
