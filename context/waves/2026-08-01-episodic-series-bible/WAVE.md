# Wave: Episodic series bible

## Goal

Establish REEL's episodic-series grammar by developing a complete first-season
bible for *Reading the Runes*, sourced from BANISH's catastrophe-inference
candidate without changing upstream scenario canon.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Format and series bible | active | Add episodic-series grammar, work brief, cast, Houses, cultures, season board, continuity ledger, and Atlantis pilot treatment. |
| 02 | Story panel | planned | Review season rhythm, emotional continuity, episodic legibility, and guest-culture agency through REEL roles. |
| 03 | Pilot beat sheet | planned | Convert the Atlantis two-parter treatment into scene-level dramatic beats. |
| 04 | Season continuity packet | planned | Separate character, House, evidence, time-cost, rescue, and mythology state into production-ready ledgers. |
| 05 | Pilot animatic brief | planned | Define a short proof sequence and only then research renderer/style choices. |

## Success criteria

- Each episode changes crew, Council, and mystery state.
- Every House has a defensible public argument and a self-interested private move.
- Every ancient culture has an internal POV, ordinary-life texture, and more than
  one serious interpretation of the signs.
- The finale pays off evidence planted in the Atlantis pilot.
- House Flip's destination-conditioning plot is reconstructable episode by
  episode before its reveal.
- The Council scoreboard moves from comedy to institutional danger without
  losing its comic function.
- The first season answers what the present event is while preserving a new
  second-season objective.

## Non-goals

- No screenplay, shot list, manifest, casting, final art direction, renderer, or
  provider selection in this wave's first pulse.
- No edits to BANISH or TIGRIS canon from inside REEL.
- No large generated media.
- No imitation of the named creative touchstones' protected characters, worlds,
  dialogue, production design, or episode plots.

## Validation

```powershell
git grep -n "REEL" -- README.md PRODUCT_PLAN.md context\waves\PHASES.md
git diff --check
cargo test --quiet
```

