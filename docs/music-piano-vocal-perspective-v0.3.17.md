# Piano/vocal score and dual-perspective comparison in CLI v0.3.17

REEL v0.3.17 adds two opt-in capabilities to the existing music-reconstruction
line: measured piano/vocal MusicXML and a deterministic comparison between a
recovered melody model and a piano-reduction melody model. Existing models,
score plans, packets, and lead-sheet SVG bytes remain unchanged when the new
field is absent.

REEL still does not separate a recording, transcribe notes, run a piano-cover
model, invent accompaniment, choose an arrangement, or approve a score. Those
operations remain external, governed inputs. Model/checkpoint, parameters,
license, network policy, uncertainty, and imported artifact hashes belong in
the existing source, interchange, and analysis evidence chain.

## Measured piano/vocal MusicXML

The optional `piano_vocal_score` section of `reel.music-model.v0.1` declares:

```yaml
piano_vocal_score:
  title: Synthetic piano and vocal score
  vocal_part_id: vocal
  piano_right_hand_part_id: piano-right
  piano_left_hand_part_id: piano-left
  pickup_ticks: 0
```

All three parts must exist, be non-empty, and be distinct. The vocal part must
have `vocal` or `melody` role and must equal the existing lead-sheet melody
part, keeping lyric underlay under the same exact authority binding. A nonzero
pickup must be shorter than the first complete measure. Meter changes must
occur on resolved measure boundaries.

`music-score-export-plan` adds `piano-vocal.musicxml` only for a model that
declares this section. The artifact contains a measured vocal staff and one
two-staff piano part with treble and bass clefs, rests, voices, bar-crossing
ties, tempo and form directions, harmony symbols, and exact lyric underlay.
The existing `score.musicxml`, MIDI, guide WAV, and optional lead-sheet SVG
remain intact. The receipt regenerates and byte-compares the measured score,
records its note and measure census, and rejects tampering or overwrite.

The MusicXML is the editable authority. A notation application may produce PDF
or another printable derivative, but REEL v0.3.17 does not bundle or silently
invoke MuseScore, LilyPond, or a remote engraver. Engraving, page turns,
enharmonic spelling, fingering, singability, and two-hand playability require
human review.

## Recovered-versus-piano comparison

An owner-authored `reel.music-perspective-comparison.v0.1` manifest binds the
exact recovered model and exact piano model, selects one melody/vocal part from
each, and declares integer onset, duration, and pitch tolerances. Both models
must use the same musical timebase and duration.

```powershell
reel music-perspective-compare comparison.yaml `
  --output-path perspective-report.json --output json
reel music-perspective-check comparison.yaml perspective-report.json `
  --output json
```

Candidate note pairs are ordered deterministically by pitch difference, onset
difference, duration difference, and stable note IDs. Each note may match at
most once. The report separates exact matches, tolerance matches, recovered-
only notes, and piano-only notes; it also reports structural tempo, meter, and
form agreement without confusing different provenance records with musical
disagreement. Agreement is a simple matched-note coverage ratio in integer
millionths, not a creative score or confidence claim.

The report contains no filesystem paths and is marked non-shareable because
model and note identities may still be private. The checker regenerates the
complete report and rejects changed inputs, bindings, policy, or report bytes.

## Intended workflow

1. Build and human-correct a model from the original recording and its admitted
   separation/transcription evidence.
2. Produce a piano-reduction candidate with an owner-selected local or approved
   external model; admit its audio/MIDI and exact generation provenance.
3. Build a second corrected melody model from the piano reduction.
4. Compare both models with explicit tolerances. Agreement increases review
   confidence; disagreement becomes a correction target and is never averaged
   away automatically.
5. Create a corrected combined model with vocal and authored piano-hand parts,
   then export the piano/vocal score packet.
6. A designated human arranger selects or corrects the candidate and separately
   approves playability and musical identity.

## Phased omissions

- No embedded Pop2Piano, AccoMontage2, PiCoGen, separator, or transcription
  runtime; REEL accepts their governed outputs through existing adapters.
- No automatic accompaniment generation or automatic reconciliation of the two
  perspectives.
- No bundled PDF renderer. A future hash-pinned local notation adapter can add
  PDF/SVG without making either format the editable authority.
- No claim that a piano reduction is statistically independent evidence of the
  original recording.
