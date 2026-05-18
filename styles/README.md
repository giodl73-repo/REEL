# Animation styles

REEL separates **format** from **style**. Format says what kind of video this is;
style says how motion, camera, art direction, and rendering should behave.

Every video package should name one primary style and optional alternates before
renderer selection.

| Style | Best for | Notes |
|---|---|---|
| `storyboard-animatic` | Fast scenario proof, trailer timing, narration tests | Timed boards, camera moves, captions, scratch audio. |
| `motion-graphics` | Explainers, systems, stats, diagrams | Works well with Remotion, browser capture, SVG/canvas, and FFmpeg. |
| `cutout-2d` | Character scenes with reusable rigs | Good for dialogue and scenario explainers without full frame animation. |
| `illustrated-2d` | Mood-forward shorts and trailers | Strong style control; requires continuity references. |
| `pixel-art` | Retro/game scenarios | Useful for strategy, sim, and arcade-like scenario clips. |
| `isometric-game` | Settlements, maps, logistics, tactical scenes | Natural fit for BANISH-style world/system scenarios. |
| `3d-previs` | Blocking, camera, spatial continuity | Can start rough before final art direction. |
| `cinematic-ai` | Prompt-driven generated scenes | Requires strict continuity, shot, and style constraints. |

## Games Design scenario adaptation

When adapting a Games Design scenario, REEL should preserve:

1. Source repo and scenario id.
2. Character, place, faction, object, and event continuity.
3. Selected format and style.
4. Shot-level camera, action, and visual prompt notes.
5. Sound, music, narration, captions, and export targets.

REEL may create variants of the same scenario in different styles, but it should
not silently change the scenario canon owned by the source game repo.
