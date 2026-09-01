# Governed semantic import in CLI v0.3.4

REEL v0.3.4 adds `reel.music-semantic-import.v0.1`,
`music-semantic-import-validate`, and `music-semantic-import-write`. This is the
first typed bridge from existing-tool interchange artifacts into
`reel.music-analysis.v0.1` observations.

## What the bridge does

An external local adapter interprets its tool's native CSV, MIDI, MusicXML,
JAMS, or other admitted artifact and writes a strict normalized semantic
sidecar. REEL does not pretend that all native formats share one parser. It
validates the sidecar against:

- the exact byte and canonical hashes of a C3 interchange intake;
- the exact byte and canonical hashes of a C4 comparison;
- an explicitly selected candidate and its separately referenced decision;
- the selected artifact's purpose;
- exact adapter executable, version, parameters, model, license, and denied-
  network provenance; and
- the immutable source audio and musical timebases inherited through intake.

Only note-event, feature-annotation, and score-candidate artifacts can enter the
v0.1 semantic event bridge. Stems, sonifications, and raw model arrays require
different typed downstream contracts.

## Exact time mapping

Every normalized event retains its native source locator and declares one
integer time representation:

- sample indices plus the originating sample rate;
- integer microseconds; or
- musical ticks plus PPQ and microseconds per quarter note.

It also declares the expected half-open range in immutable-source samples.
REEL recomputes that range with checked integer rational arithmetic and the
source contract's rounding policy. Floating-point seconds are never accepted.
Out-of-range, empty, overflowing, or incorrectly rounded mappings fail.

## Analysis promotion and lineage

`music-semantic-import-write` atomically writes a new analysis manifest and
refuses overwrite. The generated analysis binds the semantic-import bytes and
canonical contract. Its analyzer binds the import ID, and every observation
binds exactly one import event. Analysis validation revalidates the full chain
and requires imported values, confidence, uncertainty, and sample ranges to
match exactly. Every bound import event must appear once—no silent cherry-
picking or duplication.

The output and bound artifacts must share a filesystem root so the generated
manifest can retain portable relative paths. Cross-volume Windows output is
rejected instead of embedding a machine-specific absolute path.

These optional import fields are omitted from older analysis serialization, so
the existing `reel.music-analysis.v0.1` fixture hashes and model bindings remain
compatible. A corrected music model can cite the generated analysis
observations through its existing evidence references. Imported observations
remain provisional evidence; promotion does not make them correct, selected for
release, human-approved, or equivalent to original multitracks.

## Example

```powershell
cargo run -- music-semantic-import-validate `
  manifests/fixtures/music-interchange-intake/semantic-import.yaml `
  --output json

cargo run -- music-semantic-import-write `
  manifests/fixtures/music-interchange-intake/semantic-import.yaml `
  --output-path production/private/imported-analysis.yaml `
  --output json
```

The checked fixture is synthetic and uses sub-second rows compatible with its
tiny source. It demonstrates microsecond-to-sample mapping and tests also cover
sample-rate and musical-tick mapping. It is not evidence that a particular
third-party parser profile is correct.

## Explicit non-goals

- No bundled parser for a named third-party tool.
- No analyzer, model, decoder, network request, or remote service execution.
- No automatic candidate selection or correction closure.
- No conversion of stems or raw model arrays into musical truth.
- No automatic corrected-model authoring, translation, repair, arrangement, or
  release approval.
- No shareable receipt; validation and write reports remain private.

Real tool-specific adapters require sanitized outputs from the operator
workflow, independently reviewed column/namespace semantics, and additional
profile-specific tests.
