# Privacy-safe animatic receipts in CLI v0.2.6

Full `*.artifacts.json` reports are local verification records. They retain the
resolved paths and hashes needed to recheck manifests, captions, audio, and
visual inputs. Those reports should not be shared casually when their paths or
input identities are private.

REEL CLI v0.2.6 adds a separate, narrow sharing derivative without changing
`reel.manifest.v0.2`:

```text
reel animatic-receipt candidate-v026.artifacts.json \
  --output candidate-v026.receipt.json \
  --format json
```

The command first runs the full local `animatic-check`. Only after verification
passes does it atomically publish `reel.animatic-receipt.v0.1`. Existing output
is never overwritten.

The receipt contains:

- a hash of the complete local source artifact report;
- the verified video hash and byte length;
- dimensions, frame rate, duration, silent/audio state, and caption count;
- generic input-kind counts without IDs or per-input hashes;
- normalized motion backend, quality, interpolation, curve, shot count, and
  aggregate safety result;
- native/WSL transport and the render-environment fingerprint.

It omits work ID and title, filenames, local paths, artifact/output locations,
input IDs, per-input hashes, executable version strings, shot IDs, manuscript
references, people, and approval records. Unknown input kinds are counted as
`other` rather than serialized verbatim.

The receipt is safe for intentional operational sharing under this path-free
contract. It is not a signature, release approval, consent record, or direction
to publish the underlying video. Keep the complete artifact report local for
reverification.
