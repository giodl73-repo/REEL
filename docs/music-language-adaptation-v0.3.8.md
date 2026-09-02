# Same-music language adaptation in CLI v0.3.8

REEL v0.3.8 adds `reel.music-language-adaptation.v0.1` and
`music-language-adaptation-check`. This is a planning and lineage contract for
fitting an approved target-language text to an already governed composition. It
does not translate text, synthesize a voice, render audio, or approve wording.

## Exact inherited music

An adaptation recursively validates the exact C6 model draft and its upstream
analysis/source lineage. `preserved_model_targets` must name every governed
tempo, meter, form, note, harmony, rhythm, and hook target exactly once. The
bound raw-PCM accompaniment must match its file and decoded hashes, source
contract and derivation decision, byte count, format, and the exact duration
derived from the model tempo map.

This makes “same music” a checked inheritance statement, not a title or reviewer
impression. A later performance may cite this plan; it cannot silently omit or
replace a governed musical element.

## Separate source and target text

The canonical-source and approved-target text layers each bind exact UTF-8 file
bytes, a language, authority, and ordered byte-range units. Units must cover
every non-whitespace character exactly once without overlap. Source and target
languages must differ, and only the target layer may use the `approved` authority
status; that status requires an immutable decision reference.

Ordered translation links must consume every source unit and every target unit
exactly once. They record alignment and rationale, not semantic equivalence.
Translation authority remains with the producing project and its actual human
reviewers.

## Note underlay and exceptions

Every target unit receives an ordered melody/vocal-note underlay with a stress
classification and melisma count. Note references must resolve in the inherited
model, musical time cannot move backward, and melisma counts must match the
number of cited notes.

A source/target link whose unit counts differ requires a prosody exception.
Exceptions identify their translation link, target units, governed notes, typed
change, rationale, accountable review roles, and decision. They may document
duration, onset, pitch, melisma, stress, rest, pickup, cadence, or phrase-boundary
changes; they do not rewrite the inherited model or grant musical approval.

## Synthetic proof

The integration fixture uses the invented source text `lu na bri lla` and target
text `the moon is shin ing` with a generated four-second, 8 kHz mono raw-PCM
accompaniment. No private lyrics or audio are checked into REEL. The valid plan
is checked with:

```powershell
cargo run -- music-language-adaptation-check adaptation.yaml --output json
```

Tamper tests reject changed target text, incomplete translation coverage,
missing model targets, accompaniment-duration drift, unknown underlay notes,
missing required prosody exceptions, and target text lacking approved authority.

## Boundary

The resulting report is private and path-free. It confirms contract integrity,
not translation quality, singability, performance quality, consent, selection,
delivery, publication, or release. A target-language performance and bilingual
listening comparison remain a separate future contract.
