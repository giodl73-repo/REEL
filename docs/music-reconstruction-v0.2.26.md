# Music-reconstruction foundation in CLI v0.2.26

REEL v0.2.26 adds the `reel-music` library crate and the first deterministic
contracts from the reviewed music-reconstruction proposal. The root `reel`
binary remains the only public CLI and process-integration surface.

```powershell
reel music-source-validate source.yaml --output json
reel music-neutral-plan source.yaml --output-path neutral.json --output json
reel music-neutral-check neutral.json source.yaml candidate.raw --output json
reel music-repair-plan repair.yaml --output json
```

## Implemented contracts

### `reel.music-source.v0.1`

The foundation accepts explicitly formatted raw PCM (`u8`, signed 16/24/32-bit
little-endian, or 32-bit float little-endian). It verifies:

- exact file SHA-256 and normalized decoded-PCM SHA-256;
- byte length against sample rate, channels, sample count, and sample format;
- integer sample and musical PPQ timebases with explicit rounding;
- authority namespace, artifact/content identity, review roles, and optional
  immutable decision references; and
- private, network-denied, no-third-party-upload egress.

Raw PCM makes decoded identity independently testable without selecting a media
decoder in the core crate. Container decoding and normalized fingerprints are a
later explicit adapter slice.

### `reel.music-neutral-plan.v0.1`

`music-neutral-plan` writes a new JSON derivative atomically and refuses to
overwrite an existing plan. It contains one full-range `keep`, one full-range
lock, the exact source-manifest and canonical-contract hashes, the decoded-PCM
hash, and the complete audio timebase.

`music-neutral-check` revalidates the current source and plan, then requires the
candidate raw PCM to match the decoded source hash and byte length exactly. It
does not render or normalize the candidate.

### `reel.music-repair.v0.1`

The typed planning grammar includes `keep`, `cut`, `insert`, `replace`,
`repeat`, `move`, `crossfade`, `preserve-tail`, `match-gain`, `match-eq`,
`extend-bars`, and `lock`. Validation requires:

- source-manifest, source-contract, decoded-signal, and timebase agreement;
- unique operation IDs and valid half-open sample ranges;
- hash-verified external assets for insert or replacement;
- raw/decoded asset identity, byte length, format, sample-rate/channel
  compatibility, bounded asset ranges, and equal destination duration;
- ordered nonoverlapping changed envelopes and locks;
- no changed/locked intersection;
- every source sample covered exactly once by either a change or a lock;
- every mutating operation contained in a changed envelope;
- every changed envelope fully covered by mutating operations; and
- the music-reconstruction, sound, edit, and rights/provenance roles.

Governed `reviewed`, `approved`, `selected`, or `released` statuses require
immutable decision references; a status string cannot stand in for evidence.
All v0.2.26 validation and plan reports explicitly declare `shareable: false`.
They contain local IDs, paths, or hashes and are not privacy-safe exchange
receipts.

Overlapping mutating operations are rejected in this foundation. A later
contract may add explicit operation groups with defined composition semantics.

## Canonical identity and portability

Contracts receive a canonical JSON hash after strict deserialization. Map keys
are sorted recursively and array order is preserved, so YAML key order does not
change semantic identity. Tests keep raw-manifest and canonical-contract hashes
separate and assert the checked-in synthetic fixture's canonical hash.

Text fixtures are pinned to LF and raw PCM fixtures are marked binary in
`.gitattributes`. This corrects the v0.2.25 Windows failure in which an exact-
lyric fixture's LF hash was checked against CRLF worktree bytes.

## Synthetic evidence

`manifests/fixtures/music-repair-foundation/` contains only generated unsigned
8-bit sample bytes and generic identifiers. Its repair marks one range changed
and locks every outside sample. Unit and CLI tests also generate temporary PCM
and prove:

- stale source hashes fail;
- semantic contract hashes survive YAML key reordering;
- neutral candidates must be byte-identical decoded PCM;
- lock trespass fails;
- overlapping mutating operations fail; and
- required roles and complete changed/locked coverage are enforced.
- inferred approval and unknown operation fields are rejected.

No BERTICA audio, lyrics, paths, titles, identity, or creative judgment appears
in the crate, fixture, or tests.

## Boundary and next slice

This release validates plans and evidence only. It does not yet decode
containers, render an edit decision list, measure acoustic seams, separate or
transcribe stems, export notation, adapt language, re-orchestrate a score, or
invoke ACE. Slice B will resolve a validated repair plan into a deterministic
edit list, render the synthetic repair through the existing FFmpeg boundary,
and verify exact outside-region identity plus seam/tail evidence.
