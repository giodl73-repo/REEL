# Shared-video receipt verification in CLI v0.2.7

`animatic-receipt-check` lets a recipient verify an intentionally shared video
against its privacy-safe receipt without receiving the producer's private
artifact report:

```text
reel animatic-receipt-check candidate-v027.receipt.json \
  candidate-v027.mp4 --output json
```

The command strictly decodes `reel.animatic-receipt.v0.1`; unknown fields,
including an injected `path`, are rejected. It validates the receipt schemas,
minimum tool version, normalized motion facts, input counts, transport, hash
shapes, and safety state. It then hashes and probes the shared video and checks:

- SHA-256 and byte length;
- one H.264/yuv420p video stream;
- dimensions and constant frame rate;
- duration within one video frame; and
- the expected audio-stream state.

Successful JSON output uses `reel.animatic-receipt-check.v0.1` and contains only
hashes and measured delivery facts—no filenames, paths, work identity, or input
details.

## Trust boundary

The check proves that the inspected video matches the receipt's delivery facts.
Without the private artifact report it cannot independently validate the
receipt's source-artifact hash or caption wording, and it does not establish an
author, signature, consent, release, approval, or direction to publish. Signing
and organizational approval remain separate concerns.

This is CLI and derivative-report behavior only. `reel.manifest.v0.2` is
unchanged and existing manifests require no migration.
