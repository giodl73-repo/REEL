# Bounded external music-repair adapter v0.3.15

REEL v0.3.15 provides a governed file boundary for optional re-singing or
repainting after deterministic diagnosis. It does not call ACE or another
engine, install a model, download weights, upload media, synthesize a voice, or
choose a candidate.

```powershell
reel music-external-repair-validate request.yaml --output json
reel music-external-repair-plan request.yaml `
  --receipt request.receipt.json --output json
reel music-external-repair-candidate-check candidate.yaml --output json
```

## Request contract

`reel.music-external-repair-request.v0.1` binds:

- the exact repair manifest, contract, operation, and half-open sample region;
- the target text file, its content hash, language, performance mode, and
  separate exact-text authority hash;
- the retained music file and hash;
- adapter kind/version/executable, model ID, checkpoint hash, license, seed,
  and parameters;
- local-only, network-denied, no-auto-download execution policy;
- recorded speaker-specific voice-consent evidence; and
- explicit boundary, regional loudness, and lyric-coverage thresholds.

The validator forbids third-party upload and public-release authority. The plan
receipt is path-free and contains no target text. It explicitly records that
independent lyric evidence and human listening remain required, and that
nothing has been selected or released.

## Candidate contract

A candidate is a complete raw-PCM timeline with the exact source format,
sample rate, channels, and duration. REEL independently recomputes:

- exact byte identity before and after the requested region;
- boundary delta at both region edges;
- regional loudness delta against the source; and
- all request, plan, source, candidate, and policy hashes.

Lyric evidence is a separate hash-bound artifact. It must bind the candidate
and target-text hashes, name an analyzer different from the generation adapter,
state coverage, and report whether the performed text matched. The input
request is never treated as evidence of what was actually sung.

Only a technically passing candidate can be marked `audition-ready`, and that
state requires a disposition record. A failed generation may be retained as
`rejected` with its own disposition evidence. Neither state means selected,
approved, delivered, published, or released.
