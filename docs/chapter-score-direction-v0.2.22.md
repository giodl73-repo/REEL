# Chapter score direction — CLI v0.2.22

REEL can now own the creative intent of a score without pretending that a
manifest is itself a composer. The additive `score` block captures a movie's
musical arc in structured, portable terms while leaving synthesis, performance,
licensing, and listening approval to the appropriate downstream workflow.

Existing `reel.manifest.v0.2` files require no migration. `score` is optional.

## What the contract expresses

- originality policy: `original-only`, `licensed`, or `temp-review`;
- a global creative brief and explicit avoid list;
- reusable motifs and how they recur;
- global and cue-local instrument families, roles, timbres, and articulations;
- chapter, narrative function, mood movement, energy movement, tempo, and meter;
- style tags as descriptive palettes, not artist-imitation requests;
- transition-in/out language, montage intent, and picture-edit notes;
- exact score sync points, optionally bound to manifest beat markers.

The contract deliberately does not claim that a style tag creates an authentic
regional genre, that a generated performance is licensed, or that a numeric
energy curve substitutes for human listening.

## Example

```yaml
score:
  originality_policy: original-only
  creative_brief: Carry one persistence motif through changing city palettes.
  global_instruments:
    - { family: brass, role: recurring identity, timbre: warm and human }
  motifs:
    - { id: persistence, description: rising three-note idea, instruments: [brass, piano] }
  avoid: [copyrighted melodies, artist imitation]
  cues:
    - id: desert-lift
      start_seconds: 42.0
      duration_seconds: 38.0
      chapter: Palm Desert
      narrative_function: first professional momentum
      mood_from: displaced
      mood_to: playful
      energy_from: 0.3
      energy_to: 0.8
      tempo_bpm: 108
      style_tags: [sunlit, desert, montage]
      instruments:
        - { family: hand-percussion, role: pulse, timbre: dry and close }
        - { family: plucked-strings, role: hook, timbre: airy }
      motif_ids: [persistence]
      transition_in: begin on the arrival image
      montage_intent: preserve complete scoring calls
      sync_points:
        - { id: arrival, time_seconds: 42.0, kind: transition, beat_marker_id: desert, emphasis: 0.9 }
```

## Validation

Timed manifests require cue start and duration. REEL rejects cues outside the
timeline, energy/emphasis outside `0..1`, tempos outside `20..320` BPM, unknown
motifs or beat markers, sync points outside their cue, and beat-bound sync
points that do not align exactly. At least one global or cue-local instrument
direction is required when a score block exists.

## Score-plan handoff

```powershell
reel score-plan manifest.yaml --output json
```

The command validates the production manifest and emits
`reel.score-plan.v0.1`: a deterministic, renderer/provider-neutral packet with
the score brief, motifs, instruments, cue timing, energy, transitions, montage
notes, and sync points. Text output provides a compact cue rundown.

The packet is direction, not evidence of execution. A future adapter can bind
provider inputs and rendered stems to this plan without weakening that boundary.
