# Deterministic music score export in CLI v0.3.1

REEL v0.3.1 implements music-reconstruction Slice C2. It exports a validated
`reel.music-model.v0.1` contract through local, provider-neutral adapters. It
does not run an analyzer, infer corrections, translate lyrics, choose an
arrangement, contact a network service, or approve a performance.

```powershell
reel music-score-export-plan model.yaml --output-path export-plan.json --output json
reel music-score-export-render export-plan.json model.yaml `
  --output-dir score-packet --output json
reel music-score-export-check score-packet/receipt.json export-plan.json `
  model.yaml score-packet --output json
```

## Plan and packet

`reel.music-score-export-plan.v0.1` binds the exact model bytes and canonical
model contract. It fixes exact model-tick quantization and requests three
artifacts:

- `score.mid`, Standard MIDI File format 1 with a conductor track and one track
  per editable part;
- `score.musicxml`, a score-partwise MusicXML 4.0 document with divisions,
  parts, voices, forward/backup timing, pitches, durations, tempo directions,
  meter declarations, and rehearsal marks; and
- `rehearsal-guide.wav`, mono 48 kHz signed 16-bit PCM generated from the first
  melody or vocal part, falling back to the first part.

The guide uses a deliberately plain, band-unlimited square wave with integer
phase increments. It is useful for checking entry, duration, rough pitch, and
tempo. It is not a proposed timbre, performance, mix, score, or master.

The renderer refuses to overwrite an output directory. It writes into a sibling
temporary directory, validates the complete packet, and renames the directory
into place only after every check passes.

## Independent round trip

`reel.music-score-export-receipt.v0.1` records the plan and model identities,
adapter versions, exact artifact hashes and byte counts, and comparison results.
The checker parses the exported MIDI event stream and MusicXML note/direction
elements back into independent structural snapshots. Both must equal the model
for:

- PPQ and complete duration;
- every tempo and meter event;
- form-section start, ID, and label;
- part, voice, onset, duration, MIDI pitch, and velocity for every note; and
- lyric-layer ID, kind, language, and exact content hash.

Changing an exported pitch, timing event, marker, lyric binding, file hash, or
receipt causes verification to fail. Lyric text is not embedded: Slice C2
preserves exact layer identities and hashes because the current corrected model
does not yet define syllable-to-note underlay.

## Capability boundary

MusicXML v0.1 requires non-overlapping notes inside each part voice and MIDI
voices 1 through 16. The MusicXML remains editable, but its single implicit,
non-controlling measure prioritizes exact tick preservation over engraved page
layout. The round trip does not prove readable engraving, instrumental range,
singability, playability, recognizable musical identity, or emotional fidelity.
Those require human score/arrangement and sound review.

The plan and receipt are marked `shareable: false`. They retain private model
and artifact hashes and local filenames. Technical verification is distinct
from listening review, candidate selection, rights clearance, human approval,
delivery, and publication authorization.

The checked-in synthetic fixture contains no private BERTICA material. CLI and
tamper tests prove atomic publication, deterministic guide bytes, independent
MIDI and MusicXML note re-import, and rejection after an exported pitch changes.
