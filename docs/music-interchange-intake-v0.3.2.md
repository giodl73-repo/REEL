# Existing-tool music interchange intake in CLI v0.3.2

REEL v0.3.2 accepts local artifacts people already produce with separation,
transcription, feature-analysis, and notation tools. REEL does not execute,
download, wrap, or replace those tools.

```powershell
reel music-interchange-validate intake.yaml --output json
```

`reel.music-interchange-intake.v0.1` binds the immutable REEL source, scoped
authority, every external producer, exact artifact bytes, declared purpose and
format, semantic roles, uncertainty, limitations, and review state. Producers
record adapter, version, executable hash, parameter hash, software license,
dataset disclosure, and denied-network policy. Model revision, exact model hash,
and model license are an all-or-none group for model-backed tools.

## Accepted existing outputs

| Purpose | Accepted formats |
|---|---|
| Stem estimate | WAV, FLAC |
| Note events | MIDI, CSV |
| Feature annotations | CSV, lab, JAMS, RDF, MIDI |
| Score candidate | MIDI, MusicXML |
| Raw model output | NPZ |
| Sonification | WAV, FLAC |

Validation checks actual file bytes rather than trusting extensions: RIFF/WAVE,
FLAC, SMF, MusicXML score roots, delimited UTF-8 text, JAMS JSON structure, RDF
text, and NPZ ZIP signatures. This is container-shape validation, not a claim
that the contents are musically correct or approved.

## Stem normalization

Container stems must bind a separate deterministic raw-PCM normalization. The
normalization records decoder identity/version, parameter hash, raw format,
timebase, decoded hash, and denied-network policy. Exact raw bytes must match
their declared length and the immutable source timebase. This makes existing
WAV/FLAC outputs eligible for the current `reel.music-analysis.v0.1` stem
evidence without describing separator estimates as original multitracks.

REEL v0.3.2 validates the normalization but does not perform it. The producing
project remains responsible for its local decoder invocation and for entering
mixture-consistency, bleed, confidence, observation semantics, and uncertainty
in the later analysis contract.

## Privacy and authority boundary

The validation command performs no network request, tool execution, model
loading, conversion, analysis, or upload. The report is marked
`shareable: false` because artifact and source hashes can identify private
material.

Successful intake proves identity, lineage, declared format shape, and local
policy. It does not prove transcription accuracy, separation quality,
annotation correctness, score usefulness, rights clearance, human approval,
candidate selection, or publication authorization.

The checked fixture contains only synthetic CSV and JAMS examples. It uses
compatibility-shaped data and does not bundle third-party code, model output,
private audio, BERTICA material, or claims that an external application created
the fixture.
