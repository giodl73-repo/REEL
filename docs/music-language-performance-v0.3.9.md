# Target-language performance and bilingual comparison in CLI v0.3.9

REEL v0.3.9 adds `reel.music-language-performance.v0.1` and
`music-language-performance-check`. It binds one target-language vocal take and
one bilingual comparison record to an exact, recursively validated v0.3.8
adaptation. It does not generate, transcribe, listen to, select, deliver, or
release a performance.

## Exact performance candidate

The manifest binds the C9 adaptation by file hash, canonical contract hash, and
identity. The vocal take must be raw PCM with independently matching file and
decoded hashes, and must use the accompaniment's exact format, channel count,
sample rate, and duration. This proves candidate identity and temporal fit; it
does not prove that requested words were sung.

The separately hashed as-performed transcript therefore has its own language,
authority, and complete ordered UTF-8 unit coverage. Every approved target unit
has exactly one audit disposition:

- `matched` requires one byte-exact performed unit and forbids a decision;
- `changed` requires performed text and an immutable decision;
- `omitted` forbids performed text and requires an immutable decision; and
- `uncertain` remains explicit and requires an immutable decision.

The audit must also consume every performed-text unit exactly once and in order.
A separate lyric-listening gate records whether an actual reviewer accepted what
was heard; input text and structural coverage cannot self-pass that gate.

## Provenance and consent

Performance provenance distinguishes human recording, synthetic voice, and the
non-identifiable fixture tone. It records the creating adapter/version, optional
seed, model/checkpoint hash and license for synthetic voice, and whether creation
was local-private or explicitly approved for external egress.

Consent is operation-specific. Its subject, operation, service/runtime,
audience, retention, and reuse scope are mandatory. Human and synthetic voices
cannot use the fixture-only exemption. Selection requires granted consent, or
the explicit non-identifiable fixture status used solely by synthetic tests.

## Bilingual comparison

The comparison binds an authority-governed source-language PCM reference to the
same corrected-model contract and exact duration/format as the target adaptation.
Distinct labels identify the source and target variants. Listening must cover
all five declared dimensions exactly once:

1. lyric fidelity;
2. prosody;
3. composition recognition;
4. accompaniment continuity; and
5. mix balance.

The completed comparison decision remains separate from the lyric-listening
decision. It records a human judgment; REEL does not infer it from hashes,
waveforms, the approved translation, or the candidate request.

## Selection and failure retention

Selection requires passing lyric listening, passing bilingual comparison,
satisfied consent, and complete technical validation. Pending states forbid
decisions; completed states require them. Rejection requires completed listening
or denied consent. The candidate authority status must agree exactly with
`candidate`, `selected`, or `rejected`, so a failed take remains an auditable
artifact instead of disappearing or being relabeled.

## Synthetic proof

The v0.3.9 test reuses the invented C9 text and creates two four-second, 8 kHz
mono raw-PCM tones in a temporary directory. One stands in for the target vocal
take and one for the source-language reference; neither is speech or a real
person's voice. No audio is checked into git.

```powershell
cargo run -- music-language-performance-check performance.yaml --output json
```

Tamper tests reject changed audio or transcript bytes, stale adaptation/model
bindings, duration drift, incomplete or decisionless lyric audit, missing model
provenance, unapproved external egress, consent shortcuts, incomplete comparison
dimensions, duplicate labels, selection before listening, and incomplete role
routing. A failed lyric-listening record remains valid only as an explicit
rejection.

## Boundary

The report is private and path-free. It never grants translation approval,
speaker consent, performance approval, creative selection, delivery permission,
publication, or release. Actual project decisions must bind the exact private
candidate and remain outside simulated role review.
