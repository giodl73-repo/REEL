# Presentation-only successors of imported scenes

An existing selected scene can receive a chapter title or a year label without
changing its original picture, dialogue, music, Sonic or VFX selection. Use an
explicit `reel.imported-scene-presentation-successor-manifest.v1` with
`presentation_successor` pointing to a hash-bound descriptor. This is a source
proof route, not a high-level scene build or creative approval.

The descriptor uses `reel.imported-scene-presentation-successor.v1` and pins:

- The unchanged original imported manifest, historical native receipt and
  cached semantic source. Historical records must be members of the original
  import's retained evidence.
- The editable template, language-local source text and compilation receipt.
- The ASS and registered font, one attachment ID, and presentation scene ID.
- The invocation's source authority and scope, and ordered source/native cue
  bindings with their original narration logical IDs.

Source cue aliases must be exact logical IDs or complete hierarchical prefixes
of the original narration ID, delimited by `.`, `-`, `:` or `/`. An alias for
one manuscript block cannot be paired with another block's take. A source text
file remains a separately selected wording/chronology assertion; this check
does not establish manuscript truth or human approval of an inferred date.

The raw derived job may change only its contract reference, append one ASS
layer, and optionally select lossless H.264 still encoding. The raw contract
may append only that full-scene title attachment. Original production bytes,
cue clocks, media sources, gains, trims, bus policies and picture controls are
preserved. Unknown raw changes are rejected before typed defaults can hide them.

Verification runs the complete original import's semantic phrase, D, primary
picture, music, Sonic and VFX clock checks. It recomputes both native plans,
checks original receipt equality, checks unchanged base spans, and recompiles
the selected template with the original native D clock. The ASS must match the
recompilation exactly. A chapter title must fit the first prose D and picture.
The descriptor currently supports native cue entrance labels; word and phrase
marker overlays require their own selected measurement evidence rather than
authored timing numbers.

Use the ordinary generic commands:

```text
reel-imported-scene-verify check-inputs <root> <manifest> --asset-root <cache>
reel-imported-scene-verify verify <root> <manifest> --asset-root <cache> --output-dir <render>
```

The resulting source proof retains original and derived job hashes plus
`presentation_successor_sha256`, distinct from `encoding_successor_sha256`.
Episode conform rechecks all source inputs, recompilation and actual native
output and compares the complete proof. Missing or partial successor fields
cannot pass by renaming a native receipt.
