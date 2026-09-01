# Governed repair candidates in CLI v0.3.7

REEL v0.3.7 adds `reel.music-repair-candidate.v0.1` and
`music-repair-candidate-check`. The contract closes the loop from governed
musical intent to exact candidate audio without allowing a technical pass to
stand in for listening or human selection.

## Recursive evidence chain

A candidate binds the exact C7 intent manifest and canonical contract, exact
candidate PCM bytes, and the exact local repair evidence report. Validation
recursively rechecks:

1. the model-bound repair intent and its full upstream model/source chain;
2. the same repair manifest named by that intent;
3. the generated EDL against the current repair and source;
4. the evidence report against the current EDL, repair, and candidate bytes;
   and
5. the candidate, evidence, and canonical contract hashes declared in the C8
   manifest.

Changing any candidate sample, repair operation, source binding, evidence
metric, adapter identity, intent, or decision invalidates the chain.

## Separate human gates

Listening and candidate selection use independent states and immutable decision
references:

- listening is `pending`, `passed`, or `failed`;
- selection is `pending`, `selected`, or `rejected`;
- completed states require decisions, while pending states forbid them;
- selection requires both passing technical evidence and passed listening; and
- rejection requires completed listening and its own rejection decision.

This means a technically failed or audibly unsuccessful candidate remains an
auditable rejected artifact. It cannot overwrite the source, disappear from the
record, or be mislabeled as selected. A technically passing candidate can also
remain pending after listening rather than being selected automatically.

## Synthetic proof

The v0.3.7 integration test generates a short periodic raw-PCM source, rebinds
the complete synthetic analysis/model/draft/intent chain, compiles the cut-only
EDL, creates the exact candidate, and records passing evidence. It then invokes:

```powershell
cargo run -- music-repair-candidate-check candidate.yaml --output json
```

The same test suite proves that candidate-byte tampering, evidence rebinding,
listening shortcuts, and selection of a technically failed candidate are
rejected. A second generated fixture retains the earlier deliberately rough
synthetic cut as an explicit failed-listening rejection.

## Boundary

Validation does not render, upload, listen, choose a preferred performance,
grant consent, approve delivery, or authorize release. Listening and selection
decisions must be recorded by the actual producing project against the exact
candidate version. Reports remain `shareable: false` because they preserve
private lineage and decision identities.
