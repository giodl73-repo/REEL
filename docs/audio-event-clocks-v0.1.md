# Assembly from an audio-event clock

Use `reel.audio-event-relative-assembly.v0.1` with the existing
`reel cue-relative-compile` command when a recording or music event supplies the
clock. The contract has the same fields as the narration-clock sidecar, but each
ordered `cue_id` must name an existing production `audio_events` ID. No invented
narration cue, text or speaker is needed. Clock durations are declared integer
samples; compilation does not measure, resample or time-scale the source audio.
It does not inherit the event's old `start_seconds`, duration, trim or loop fields
as clock evidence. The producer must supply the admitted native sample clock and
bind source placement separately.

```json
{
  "schema": "reel.audio-event-relative-assembly.v0.1",
  "id": "recording-edit",
  "production_manifest": "production.json",
  "sample_rate": 48000,
  "frame_rate": {"numerator": 25, "denominator": 1},
  "cues": [{"cue_id": "recording", "duration_samples": 5520000}],
  "attachments": []
}
```

The production manifest declares an audio event with ID `recording`, its actual
source and role. It can contain no narration cues. Attachment targets still name
existing shots, effects and audio events; caption targets use this explicit clock
namespace. Caption text/provenance, source eligibility and source-byte admission
remain separate. Audio-event clocks reject word/phrase markers rather than
claiming speech alignment. A future aligned-lyrics contract needs its own explicit
semantics and evidence.

Schema selection is strict: the original `reel.cue-relative-assembly.v0.1` binds
only narration cues, and the new schema binds only audio events. Neither falls
back to the other namespace. Duplicate audio-event IDs refuse in the new mode.
Existing Rust structs and the old serialized output remain unchanged.

The new compiled tag is `reel.audio-event-relative-conform.v0.1`. It distinguishes
clock-domain identity even when audio and narration happen to share ID strings.
Keep the exact contract, production and compiled objects with producer parameters;
a matching ID alone is not interchangeable provenance. Existing delivery planning
and rendering reuse the same picture/audio spans and sample/frame arithmetic.
Music delivery entries use their normal audio attachment and bus; the separate
dialogue-only `Audio.cue_id` field remains forbidden on music entries.

Offsets stay fixed relative to the chosen native clock when a recording changes;
they are not proportionally rescaled. Out-of-range spans refuse. Use an explicit
`cue-progress` anchor only when proportional placement is the authored intent.
Declared timing, actual audio/picture checks, current recipe correspondence,
creative approval and custody remain distinct. This compiler installs no runtime,
opens no media, creates no cache and performs no automatic render or transfer.
