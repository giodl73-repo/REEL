# Governed editable-model drafting in CLI v0.3.5

REEL v0.3.5 adds `reel.music-model-draft.v0.1` and
`music-model-draft-check`. The contract closes the governance gap between
validated analysis observations and a corrected editable song model.

## Complete observation census

A draft binds the exact model bytes and canonical contract. The model already
binds its analysis manifests, so REEL loads every observation from every bound
analysis and requires exactly one disposition for each `(analysis_id,
observation_id)` pair:

- `mapped`: the observation supports one or more named model targets;
- `omitted`: a separate immutable decision explicitly excludes it; or
- `unknown`: its uncertainty is preserved verbatim in `model.unknowns`.

There is no default disposition and manifest ordering carries no meaning.
Missing, duplicated, or foreign observation references fail.

## Bidirectional target validation

Stable target references identify model elements without relying on array
position:

- `tempo:<tick>` and `meter:<tick>`;
- `form:<id>`, `note:<id>`, `harmony:<id>`, `rhythm:<id>`, and `hook:<id>`; and
- `expressive:<note-id>`.

Each mapped target declares `observed`, `inferred`, or `human-corrected` state.
The target's actual model provenance must cite the disposition's observation
and match that state. Human-corrected mappings must carry the exact correction
decision referenced by the model; observed and inferred mappings forbid one.

Validation also runs in reverse: every analysis evidence citation anywhere in
the model must have a corresponding observation-to-target mapping. This rejects
undeclared evidence use, silent cherry-picking, and mappings that merely name a
target without matching its provenance.

## Usage

```powershell
cargo run -- music-model-draft-check `
  manifests/fixtures/music-model-corrected/draft.yaml `
  --output json
```

The fixture governs seven synthetic observations and eleven model targets. It
preserves one explicit pitch correction and distinguishes the inferred melody
continuation from observed tempo, meter, form, harmony, and rhythm evidence.
Tests also prove valid decision-bound omission and exact unknown preservation.

## Boundary

The contract validates a separately authored model; it does not invent tempo,
form, notes, lyrics, corrections, or arrangement. It does not decide whether an
observation is musically correct, whether an omission is editorially wise, or
whether the model is approved. Review, correction, selection, and release
decisions remain separate immutable artifacts. Reports remain
`shareable: false` because they retain private evidence and decision lineage.
