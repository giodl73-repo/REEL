# Score-driven limited-ensemble arrangement plans in CLI v0.3.10

REEL v0.3.10 adds `reel.music-arrangement-plan.v0.1` and
`music-arrangement-plan-check`. The plan recursively binds the corrected C6
model draft and expresses orchestration as explicit model transformations—not an
unconstrained style prompt.

## Complete musical governance

Every governed tempo, meter, form, note, harmony, rhythm, hook, and expressive
timing target must appear exactly once as `preserve`, `develop`, `replace`, or
`omit`. Preservation forbids a new decision because it promises byte-level model
semantics; every mutation or omission requires an immutable arrangement decision.

Every source part is also assigned exactly once. Non-omitted parts name one or
more instruments; omitted parts name none. Develop/replace/omit assignments are
decision-backed, so timbre reassignment cannot masquerade as unchanged source.

## Ensemble and playability

The owner sets a maximum ensemble size. Each instrument declares family,
function, MIDI range, maximum simultaneous notes, and explicit techniques.
Identifiers and techniques are unique, ranges are ordered, and the actual
ensemble cannot exceed its declared limit. No cultural instrumentation list is
hard-coded into REEL.

Every non-omitted source note maps exactly once to an assigned instrument.
Mappings must remain inside model duration, instrument range, MIDI velocity, and
polyphony limits. A preserved note must retain exact onset, duration, pitch, and
velocity. Developed or replaced mappings require decisions and remain visible.

## Candidate boundary

The plan requires every later arrangement candidate to demonstrate:

- exact plan binding and model inheritance;
- range and polyphony integrity;
- editable-score round trip;
- audible source/arrangement comparison;
- human recognition listening; and
- human selection.

C11 does not render that candidate or infer recognition. Those belong to the
next bounded contract.

## Synthetic proof

The test maps the four-note synthetic melody to one generic plucked-string voice
with a monophonic range. All eleven governed composition targets are preserved;
the part-level timbre reassignment is separately decision-backed. No audio,
private music, model download, or external service is used.

```powershell
cargo run -- music-arrangement-plan-check arrangement.yaml --output json
```

Tamper cases reject incomplete element/part/note coverage, decisionless changes,
unknown instruments, changed preserved notes, range and polyphony violations,
invalid ensemble limits, missing candidate checks, and incomplete role routing.

## Boundary

Validation proves plan completeness and structural playability. It does not
prove idiomatic technique, emotional fidelity, recognizability, session
readiness, performer consent, arrangement approval, selection, delivery, or
release. Those remain with the actual composer/music director and producing
project.
