# Local song-generation contracts in CLI v0.2.25

REEL v0.2.25 adds `reel.song-generation.v0.1`: a provider-neutral contract for
planning local, rights-gated song auditions. The first supported adapter name is
`ace-step-local`, but REEL does not bundle ACE-Step, its model weights, or a
provider-specific runtime.

```powershell
reel song-validate song.yaml --output json
reel song-engine-plan song.yaml --output-dir private/song-packet --output json
reel song-engine-plan-check private/song-packet song.yaml --output json
reel song-engine-doctor song.yaml --output json
```

## Boundary and artifacts

`song-engine-plan` validates the contract, then atomically writes:

- `request.json`: private engine input containing the exact lyrics, composition
  direction, local paths, engine identity, parameters, seed, and output request;
- `receipt.json`: path-free and lyric-free provenance containing only hashes,
  counts, engine/model/version/seed data, permissions, and review gates;
- `README.md`: a reminder that generation is neither approval nor release.

The receipt binds the manifest, lyrics, references, and request. Rechecking the
packet detects any subsequent change. The contract accepts only local-only
references and rejects stale hashes, remote egress, named-artist imitation, an
unpinned model revision, invalid duration/tempo, overlapping source ranges, and
duplicate or unsupported outputs.

## Creative and rights separation

The engine prompt describes musical characteristics, not an imitation of a
named living or historical artist. `listening_references` can preserve a human
research trail, but REEL excludes it from the engine prompt by keeping it as
review metadata.

`permissions` records the declared scope for the lyrics, voice identity,
speaker-specific voice consent evidence, third-party upload, and public
release. An assigned voice identity requires recorded consent evidence; an
original unassigned singing voice declares consent `not-applicable`. This
version requires `public_release: false`, because release is a separate human
decision. Validation never converts a rights statement into approval or
publication authority.

## Exact lyrics

`source.lyrics.exact_text: true` means the request must contain the exact bytes
bound by `sha256`. It does not claim that a generated singer pronounced or sang
every word correctly. Candidate audio needs a later listening/transcription
audit before it can be described as text-faithful.

## Engine doctor

`song-engine-doctor` is intentionally read-only. It checks whether the declared
executable can be found, the working directory exists, the model revision is
pinned, and the network policy is `offline-after-install`. It does not download
weights, access the network, or generate audio.

The sanitized fixture in `manifests/fixtures/song-generation/` contains only
synthetic lyrics and uses the locally available `cargo` executable to exercise
doctor behavior without installing a music model.
