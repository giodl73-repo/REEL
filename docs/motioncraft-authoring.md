# Motioncraft authoring and scene execution

Implementation status: phased camera authoring, compilation, actual scene
rendering and indexed review extraction are implemented on `codex/motioncraft`.
CAIMITOS adapter deployment, typography/palette direction, queue comparison,
hydrated package and final goal acceptance are still outstanding.

## Episode and scene inputs

`reel.episode-authoring.v1` accepts optional `motion_direction`, containing a
complete direction. `reel.scene-authoring.v1/v2` accepts optional
`motion_direction` with `default` and `shots` members:

```json
{
  "motion_direction": {
    "default": null,
    "shots": {
      "es": {
        "existing-spanish-event-id": {
          "purpose": "Let the viewer read before approaching",
          "dominant_element": "picture",
          "working_fps": 24,
          "duration_frames": 24,
          "reduced_motion": false,
          "elements": [{
            "id": "picture",
            "role": "camera",
            "bounds": {"x": 0.1, "y": 0.1, "width": 0.8, "height": 0.8},
            "phases": [
              {"id": "read", "kind": "hold", "start_frame": 0,
               "end_frame": 11, "curve": "linear", "zoom_from": 1, "zoom_to": 1},
              {"id": "approach", "kind": "settle", "start_frame": 12,
               "end_frame": 23, "curve": "ease-out", "zoom_from": 1, "zoom_to": 1.08}
            ]
          }]
        }
      }
    }
  }
}
```

This is a fragment to add to an existing valid scene, not a standalone scene.
Replace the example event ID with a real ID from that language's materialized
events. An English override lives under `en` and names the English event ID.
Unknown language/event IDs and unsupported properties fail validation.

Precedence is shot override, then scene default, then episode default. The
selected direction replaces the whole object; phase arrays are not merged by
position. Omitting all direction retains legacy serialization, fingerprints,
selected media and render behavior. Defaults currently carry exact authored
duration: use them only for matching spans, or author shot overrides. They do
not stretch themselves to narration. General reusable duration-fitting styles
have not yet been implemented.

Working phase spans are zero-based and inclusive. The native narration clock
remains in exact samples; the direction's whole duration must align within
half a working frame. Gaps retain the prior zoom. Adjacent phases must keep
continuous zoom; moving one-frame phases and moving `hold` phases are rejected.
Curves are linear, ease-in, ease-out and cosine ease-in-out. Reduced motion
keeps the camera at zoom 1 without changing audio or reading duration.

The first scene treatment supports one dominant element with `role: camera`
and centered zoom from 1 to 4 on an uncropped still. This is explicit renderer
capability: other roles, video sources, crops, legacy motion groups and source
frame offsets are rejected. A phase kind describes purpose, not a separate
unimplemented effect such as opacity. Typographic/shape entrances must not be
represented as if they were executed by this camera treatment.

Declared normalized element bounds must be inside the normalized safe area.
These bounds currently validate author intent; they are not a pixel detector
or a proof that a face/text remains protected during a crop. Delivery-profile
safe regions and protected geometry still need the planned integration.

## Compile and build

Use the existing selected scene authoring compile manifest with its exact
catalog, episode, scene, policy, scoped bindings, immutable graph/pointer,
verified alignment paths and delivery profile. Selected assets must be hydrated
under the public cache-relative object layout used by that manifest.

```powershell
cargo run --bin reel-scene-authoring -- compile-delivery <project-root> <compile-manifest.json>
cargo run --bin reel-scene-build -- build <project-root> <compiled-directory>/build.json --asset-root <hydrated-asset-root> --output-dir <new-render-directory>
```

Compilation resolves direction against each selected native event span. The
job contains a strict `phased-camera` compiled plan, which the scene engine
reconstructs and checks against native samples, allocated frames and supported
capabilities before rendering. Cumulative picture boundaries may allocate one
frame differently from local duration rounding; that allocation is explicit
and never modifies samples. The authored direction participates in the
language fingerprint. Scene build rejects a missing, changed or stale motion
plan even if the old delivery job still names the correct picture bytes.

Every output uses the existing no-clobber publication path and delivery
receipt. Phased transforms sample fractional rectangles with the existing
FFmpeg perspective approach and clamped per-frame curves.

## Review evidence

Directed scene outputs additionally contain:

- `motioncraft/frame-XXXXXXXX.png`: exact decoded picture frames at declared
  boundaries, their adjacent frames, hold midpoints and a uniform sample grid.
- `motioncraft/evidence.json`: global frame indices, rational timestamps,
  attachment/element/phase reasons, input and picture hashes, and producer tools.
- `motioncraft/contact-sheet.png`: those sampled frames in index order.
- `motioncraft/quarter-speed.mp4`: silent slow playback for motion inspection.
  The existing `review.mp4` retains the native audio.

Scene check verifies report identity and compares PNG pixels against decoding
the declared frame indices from the actual picture. Selection uses FFmpeg's
frame counter, not timestamp seeking. Extraction is bounded to 256 frames and
512 MiB of raw pixels per scene; oversized evidence requests fail explicitly.
Camera execution also bounds phase/expression complexity. These checks do not
prove all frames free of defects or award creative approval.

## Existing-episode upgrade and rollback

Keep the accepted episode/scene revision unchanged. Create a new explicit
authoring revision with direction while retaining its cue, event and selected
asset identities. Compile to a new output directory and build a new derivative.
Review the rendered evidence and native timing against the previous revision.
Rollback uses the previous authored revision and its existing delivery package;
no old file or render is overwritten. The implementation tests prove this
path in REEL's synthetic scene harness; CAIMITOS's own adapter adoption remains
a separate required completion gate.
