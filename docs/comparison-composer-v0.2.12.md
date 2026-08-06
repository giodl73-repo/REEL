# Controlled comparison composer in CLI v0.2.12

`comparison-compose` consumes a strict, scene-canon-independent
`reel.comparison.v0.1` YAML contract. Every variant names a video, its strict
animatic receipt, and (when creative dimensions must be compared) its local
artifact report:

```yaml
schema: reel.comparison.v0.1
id: speaker-caption-review
opening:
  title: Controlled caption review
  instructions: Compare caption presentation only. Inclusion and order are not approval.
  duration_ms: 3000
label_mode: blinded
blind_seed: local-private-seed
changed_dimension: captions
fixed_dimensions: [motion, voice, mix, visual-treatment, duration, stream-facts]
variant_slate_duration_ms: 2000
protected_silence_ms: 400
chime: local/neutral-chime.wav
replay: false
variants:
  - id: none
    video: none.mp4
    receipt: none.receipt.json
    artifact: none.artifacts.json
  - id: first-entrance
    video: first.mp4
    receipt: first.receipt.json
    artifact: first.artifacts.json
```

```powershell
reel comparison-compose comparison.yaml --output caption-review.mp4 --format json
reel comparison-receipt-check caption-review.comparison.receipt.json `
  caption-review.mp4 --output json
```

The contract supports `captions`, `motion`, `voice`, `mix`,
`visual-treatment`, `duration`, and `stream-facts` dimensions. Fixed fields are
compared from available receipt/artifact hashes and facts; missing evidence is a
failure. The declared changed dimension must actually differ across children.
All children must have distinct verified receipts/videos and compatible output
geometry. Voice/mix comparison uses independently checked stem hashes when an
audio-check report supplies them, otherwise the verified master-audio hash is
the available evidence.

Blinded labels are a deterministic seed-derived permutation of A–Z. The raw
seed and child-to-label decode remain in the local contract/artifact boundary.
The shareable strict `reel.comparison-receipt.v0.1` contains only hashes,
delivery facts, dimension declarations, child count, and verification state—no
paths, IDs, descriptive labels, decode map, instructions, preference, or
approval claim.

The composer verifies children before FFmpeg, emits a neutral opening slate and
variant slates, delays an optional local chime behind protected silence, and can
replay each variant explicitly. It normalizes delivery encoding for concat but
preserves and embeds every child receipt and checks the parent duration against
the declared slate/child total. Output, local artifact, and shareable receipt
publish as an atomic group.

Composition order, inclusion, and blind labels are review mechanics only. REEL
does not infer preference, consensus, consent, or approval. Existing
`reel.manifest.v0.2` files require no migration.
