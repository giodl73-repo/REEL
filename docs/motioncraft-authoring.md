# Motioncraft authoring and scene execution

Implementation status: phased camera authoring, compilation, actual scene
rendering and indexed review extraction shipped in Phase 1 (REEL ac296362).
Explicit native-duration fitting and transformed protected-region checks are
also implemented, with a typed visual brief carried into review evidence.
Matched study renders and a verified package/clean landscape reproduction are
available; see [study handoff](motioncraft-study-handoff.md). Phase 1 CAIMITOS
deployment and the consumer canary are recorded in that repository. Phase 2 on
`codex/motioncraft-phase-2` adds native Windows cadence, portable comparisons
and focal-anchor/pan execution; Phase 2 consumer adoption, layer validation and
complete episode qualification remain open.
The [simulated study comparison](../signals/roles/check/reel-motioncraft-queue-comparison-2026-10-05.md)
records the bounded playback/score evaluation; it is not creative approval.

## Episode and scene inputs

Planning review supports concurrent moving phases across elements and samples
their intersections at start, midpoint and end. Pairwise work is bounded to
512 moving phases per direction. The current scene camera still requires one
dominant camera element; unsupported multi-element execution fails explicitly.

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
selected media and render behavior. Duration is exact by default. Set
`fit_native_duration: true` to author an explicit reusable timing template:
its phase boundaries scale proportionally to each selected native shot's
working-frame allocation. Narration samples, cue order and selected assets
remain unchanged. The compiled plan preserves the original direction and a
separate `execution_direction` containing the resolved phases and duration.
Collapsed phases and durations outside the renderer's bounds fail explicitly;
no phase is silently discarded. Use exact shot overrides when a reading pause
needs a fixed duration rather than proportional fitting.

Working phase spans are zero-based and inclusive. The native narration clock
remains in exact samples; the direction's whole duration must align within
half a working frame. Gaps retain the prior zoom/pan. Adjacent phases must keep
continuous zoom/pan; moving one-frame phases and moving `hold` phases are rejected.
Curves are linear, ease-in, ease-out and cosine ease-in-out. Reduced motion
keeps the camera at zoom 1 and zero pan without changing audio or reading duration.

The first scene treatment supports one dominant element with `role: camera`
and bounded zoom from 1 to 4 with optional focal anchor and pan on an uncropped
still. See [the focal-camera contract](motioncraft-focal-camera-contract.md).
This is explicit renderer
capability: other roles, video sources, crops, legacy motion groups and source
frame offsets are rejected. A phase kind describes purpose, not a separate
unimplemented effect such as opacity. Typographic/shape entrances must not be
represented as if they were executed by this camera treatment.

Delivery profiles accept `motion_safe_area`, a normalized rectangle. A direction
may declare a narrower `safe_area` and `protected_regions`, an array of normalized
rectangles. Regions are authored against the contained picture canvas after
fit/padding, not against raw source pixels. Without caption reservation this
is the full delivery canvas. The camera validates transformed
regions throughout every phase; monotone bounded curves allow endpoint bounds
to prove containment across the interval. A direction cannot widen the profile
safe area. Reduced-motion validation uses its actual stationary transform.
These checks validate owner-declared geometry; identifying a face/text's actual
region remains an authoring/review responsibility. Landscape and portrait
profiles need their own correctly mapped layout and protected regions.

Delivery profiles can reuse the existing caption presentation geometry:

```json
{
  "caption_picture_layout": {
    "profile": "youtube-review",
    "layout": "reserve-caption-band"
  }
}
```

Use `phone-review` for portrait delivery. This optional profile fragment travels
into the exact selected delivery job. The existing caption style resolver
defines the picture region: 1280×520 within 1280×720 landscape, or 720×900
within 720×1280 portrait. The scene engine fits the picture into that region,
applies phased camera motion there, then pads to the full output. The reserved
band remains clear of camera motion. Captions remain a separate selected
presentation layer; reservation alone does not create or burn caption text.

With reservation, `motion_safe_area`, direction `safe_area`, element bounds and
`protected_regions` use normalized coordinates within the contained picture
region. To map a rectangle to output pixels, multiply by that region's width
and height and add its x/y offsets. They are not raw-source coordinates or
full-output percentages. Review evidence records the exact region and this
coordinate space. Use separately authored profile layouts for different aspect
ratios. Omitting the setting preserves existing jobs and review evidence.
Reserved layout rejects legacy zoompan and post-compose cameras that could
move the band; unsupported geometry fails before rendering.

## Compile and build

Directions optionally carry `visual_intent` with named `palette_roles`
(`#RRGGBB` colors), `typography_roles` (font family, relative canvas-height size,
weight), `reference_traits`, and `transition: "hard-cut"`. It records the
owner's intended hierarchy and look for review of the selected visual. It
does not regenerate or recolor selected illustrations or prove font use.
The compiled job and rendered evidence retain it for frame-by-frame comparison.
Unsupported transition execution such as a wipe is rejected. The queue proof
must demonstrate the declared look in actual assets and review frames; valid
brief syntax alone cannot satisfy the creative quality gate.

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

## Native-frame cadence evidence

After rendering, run `reel-scene-cadence <job.json> --asset-root <assets>
--render-root <existing-render> --output <new-report.json>` to reuse the
existing motion-check luma analyzer and thresholds. The command first verifies
the scene receipt, then measures exact adjacent delivery frames. Reports bind
the job and picture hashes and record the actual analyzer backend/FFmpeg
version separately from the native rendering runtime. Motioncraft cadence uses
native FFmpeg on Windows and Linux, without a WSL fallback. Set
`REEL_CADENCE_FFMPEG` to an explicit executable path, or let it resolve FFmpeg
from PATH. An invalid explicit override rejects rather than falling back.
The report records the canonical executable path and SHA-256 and rejects an
executable that changes during analysis. These local evidence reports include
machine paths; do not treat them as sanitized public receipts.
Output is no-clobber and belongs outside the render
directory. A failed check retains its report and exits nonzero.

Only an unlayered still camera treatment can grant whole-frame hold allowances.
External layers, post-compose cameras and other unsupported treatments require
separate cadence analysis and cannot pass by inheriting a camera hold. Reduced
motion and static baseline shots are checked for expected stillness. These
perceptual checks supplement indexed pixel evidence; they do not grant creative
approval.

## Existing-episode upgrade and rollback

Keep the accepted episode/scene revision unchanged. Create a new explicit
authoring revision with direction while retaining its cue, event and selected
asset identities. Compile to a new output directory and build a new derivative.
Review the rendered evidence and native timing against the previous revision.
Rollback uses the previous authored revision and its existing delivery package;
no old file or render is overwritten. The implementation tests prove this
path in REEL's synthetic scene harness; CAIMITOS's own adapter adoption remains
a separate required completion gate.
