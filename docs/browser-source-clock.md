# Browser compositions driven by measured audio

A native browser document may explicitly name `source_clock`, an alias in its
existing ID-only `inputs` map. The engine derives its exact hash just as it does
for the SVG, pictures, font and audio; authors maintain no second hash list.
Omitting this field preserves the ordinary frame/time/FPS event contract.

```json
{
  "source": "opening.svg",
  "audio": "audio.wav",
  "source_clock": "clock.json",
  "inputs": {
    "opening.svg": {"id":"picture/opening"},
    "audio.wav": {"id":"recording/theme"},
    "clock.json": {"id":"timing/theme"},
    "evidence.json": {"id":"timing/evidence"}
  },
  "width":1280,"height":720,"fps":24,"frame_count":336
}
```

The clock uses the existing `reel.presentation-source-clock.v1` vocabulary:
selected audio SHA-256, native sample rate and total samples, exact evidence
reference, and ordered named sample anchors. The full audio may be longer than
the bounded picture excerpt. The clock must cover the picture duration; anchors
stay within the source, names are unique, and samples cannot run backward.
Coincident anchors may have different names. Each boundary rounds once to the
nearest frame, with half frames rounded upward, as in still-sequence timing.

The existing persistent `reelframe` event keeps `frame`, `time` and `fps`, and
adds `sourceClock` with `sourceSha256`, `sampleRate`, `totalSamples`, `anchorIds`,
`anchorSamples` and `anchorFrames`. This passes measured data to an existing SVG
listener without rewriting its source or inserting per-artifact renderer code.
Evidence hashing establishes its subject, not human alignment or approval.

An optional `audio_window` supplies `source_samples`, `start_sample`,
`end_sample`, `fade_in_samples` and `fade_out_samples`. This exact profile
requires a single original stereo PCM stream at 48 kHz and the matching full
source clock. The selected length must equal the picture duration exactly;
fades cannot overlap. Anchors must lie in the selected window, and their frame
positions subtract its start before rounding. Other manifest behavior stays
unchanged when the window is absent.

The bounded native renderer normalizes the held full original to PCM24, checks
the full sample count, then applies the sample window and integer linear fades.
Each signed sample is multiplied by the integer numerator and divided toward
zero. The first/last faded samples are zero; this is deliberately distinct from
FFmpeg `afade`. It checks the encoded output against the enveloped PCM hash.
Normalization and envelope files are owned transient intermediates under the
existing workspace/time/process limits, never new primary assets. The legacy
unbounded renderer refuses this option rather than silently omitting it.

Capture waits for the declared fonts before the first frame. Comparison failures
report separate picture, displayed RGB, color metadata, PCM and clock booleans
without exposing private source paths or subjects. The producer contract pins
the envelope implementation as well as clock and capture code.

The generated composition manifest declares the clock as an exact asset and
points `source_clock` at that asset. Its evidence must also be declared, with
matching hash and length. Clock JSON is bounded to 64 KiB and evidence to 1 MiB;
the existing source/provider metadata budget may be stricter. Wrong audio,
changed data, missing evidence and invalid anchors refuse. Managed browser plans
validate these small controls and supported resource types before job adoption;
rendering checks their protected bytes again. JSON is served as application/json
under the unchanged no-network/no-sniff policy. Unknown resource types refuse.

Producer evidence pins the clock compiler, shared vocabulary and frame-event
implementation. Existing unclocked manifests omit the new field on serialization;
historical producer receipts retain their original tool routing. No source edit,
clock, transport check or render grants selection, release or eviction authority.

Validation includes shared-boundary/excerpt and tampering/refusal tests, existing
browser and still-timing regressions, plus an explicitly enrolled six-frame SVG
fixture. That fixture changes picture at the measured anchor and preserves the
exact PCM stream. Actual project correspondence remains separately checked.
