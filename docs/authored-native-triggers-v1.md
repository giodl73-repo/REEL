# Authored native word triggers

V2 `language_event_bindings` may carry `spoken_trigger_phrase` and an optional
one-based `spoken_trigger_occurrence`. The shared event continues to own the
picture, score, Sonic and VFX intention. Each language owns its phrase; measured
samples remain derived evidence tied to the exact recording hash.

```text
reel-scene-authoring resolve-scene-trigger-text <scene.json> <word-evidence.json> <exact-spoken-text.txt> --alignment <new-native.json> --receipt <new-receipt.json>
```

The command verifies the spoken text against the scoped cue hash, canonical cue
mapping and shared semantic anchor. It rejects missing or ambiguous phrases,
invalid occurrences and mismatched evidence. A first phrase beginning the cue
may assign its composition the leading recording silence only after matching
the phrase in both source and word evidence. A later occurrence retains its
measured entrance. No take selection or listening approval is inferred.

Optional `source_scope` on a scene and `source_text_overlay` plus
`raw_source_text_sha256` on a cue preserve source provenance. They are metadata;
the producer remains responsible for verifying the raw source and overlay.
The command verifies the exact spoken text hash, rather than trusting a filename.

## Continuous poems and quotations

`reel-aligned-split` also accepts `reel.word-aligned-line-split.v1`, with the same
manifest fields as the provider-character mode. `alignment` references exact
`reel.scene-word-timing-evidence.v1` bytes. `text` contains the ordered source
lines separated by newlines; `cue_ids` names those lines once.

The recording hash, language, first cue identity, sample rate, decoded sample
count and all line phrases must match. The source must already be mono PCM16
at the declared rate. Cuts occur at measured entrances of subsequent lines.
The first slice retains leading silence and the last retains the tail. No
resampling, stretching or repeating a whole recording for each line is allowed.
Zero-gain output slices concatenate to the source PCM exactly. Gains remain
explicit and clipping is rejected. Receipts distinguish provider-character
newlines from native word entrances.

These operations prepare reproducible native cue clocks. Normal scene delivery
still enforces the selected graph, event closure, independent language builds
and grouped visible-composition policy.
