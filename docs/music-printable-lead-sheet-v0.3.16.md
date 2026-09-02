# Printable lead-sheet export in CLI v0.3.16

REEL v0.3.16 extends the existing deterministic score packet with an optional
printable SVG lead sheet. It is an execution and evidence feature: REEL lays out
declared corrected-model content and proves what it emitted. It does not
transcribe a recording, infer lyrics or chords, choose a melody, correct a
translation, approve engraving, or authorize performance or publication.

## Corrected-model contract

`reel.music-model.v0.1` remains the schema. Its optional `lead_sheet` section
declares:

- a title and the ID of one non-empty melody or vocal part;
- an optional exact lyric-layer ID; and
- lyric-underlay items containing a stable ID, one or more source-ordered note
  IDs, a non-empty UTF-8 byte range in that lyric file, and `single`, `begin`,
  `middle`, or `end` syllabic treatment.

Validation rejects missing parts or lyric layers, duplicate or out-of-order note
references, invalid UTF-8 boundaries, overlapping or unordered text ranges,
unknown notes, and incomplete melody coverage. Lyric bytes remain controlled by
the layer's existing content hash and authority record. A model without
`lead_sheet` behaves exactly as it did before v0.3.16.

## Deterministic packet

`music-score-export-plan` adds `lead-sheet.svg` only when the model declares the
section. `music-score-export-render` produces a printable page containing:

- a treble-clef staff and declared melody notes;
- form-section labels and declared harmony symbols;
- exact syllables selected by the declared byte ranges; and
- melisma rules for multi-note underlay.

The receipt records the SVG hash and a lead-sheet comparison. The checker
recreates the SVG from the bound model and requires byte equality in addition
to the existing MIDI, MusicXML, and rehearsal-guide checks. Output-directory
overwrite protection and atomic packet publication are unchanged.

## Review and authority boundary

The SVG is a utilitarian review artifact, not an engraved creative master.
Human score/arrangement review remains responsible for notation readability,
range, phrasing, harmony presentation, and playability. The actual text or
translation authority remains responsible for lyric wording and underlay.
Technical validity never implies selection, voice consent, rights clearance,
creative approval, a Golden designation, delivery approval, or publication.

The checked-in fixture is synthetic and contains no BERTICA material.
