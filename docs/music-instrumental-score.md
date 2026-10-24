# Measured instrumental score export

The existing `reel.music-model.v0.1` has an optional `instrumental_score`
projection. Models without it retain their existing canonical serialization,
score packets and adapter output bytes. It uses the same measured-part writer as
the existing piano/vocal perspective, with no new transcription or scene grammar.

```yaml
instrumental_score:
  title: Synthetic instrumental ensemble
  pickup_ticks: 0
  key_fifths: null
  parts:
    - id: flute
      name: Flute
      staff_part_ids: [flute-source]
      clef: treble
    - id: piano
      name: Piano
      staff_part_ids: [piano-right, piano-left]
      clef: grand
    - id: percussion
      name: Percussion
      staff_part_ids: [drum-source]
      clef: percussion
```

Every nonempty source-model part must occur exactly once. A grand staff requires
two source parts; other clefs require one. Missing, duplicate, empty and silently
omitted source parts fail validation. Percussion clefs require rhythm parts.
Optional key signatures are explicit declarations within -7 to +7 fifths, never
automatic key findings. Pickups must be shorter than the first full measure.

`music-score-export-plan` requests `instrumental-score.musicxml` only for this
projection. The existing render/check commands emit measured bars, rests, tied
bar-crossing notes, tempo and form directions, harmony, bass/grand/percussion
clefs and optional key signatures. No lyrics or voice part is invented.
Percussion retains its declared MIDI note identity and uses MusicXML's one-based
`midi-unpitched` mapping. The note child order follows the
[MusicXML note specification](https://www.w3.org/2021/06/musicxml40/musicxml-reference/elements/note/);
percussion numbering follows the
[midi-unpitched specification](https://www.w3.org/2021/06/musicxml40/musicxml-reference/elements/midi-unpitched/).

The receipt checks deterministic bytes and independently parses every written
note fragment against source identities, pitches, voices, staff assignment and
interval coverage. Ties may split a source note across measures but cannot add,
drop, overlap or shorten its sounding interval. Existing MIDI, legacy MusicXML,
SVG and diagnostic-guide round trips remain unchanged.

An external notation application may engrave the measured MusicXML to PDF.
Structural/clock equality does not establish transcription accuracy, playability,
good voicing, readable engraving, performance consent or musical approval.
Transcription, notation quantization, expressive timing and arrangement choices
retain separate evidence and review states. The schematic rehearsal SVG remains
a diagnostic chart; the measured MusicXML is the printable score authority.
