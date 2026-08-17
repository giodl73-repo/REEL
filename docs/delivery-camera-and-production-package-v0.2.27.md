# Delivery camera and production packages — v0.2.27

v0.2.27 completes two boundaries left deliberately open in v0.2.26: semantic
camera choreography now affects sprite delivery renders, and a finished set of
production artifacts can be verified as one package without confusing byte
integrity with permission to publish.

## Sprite camera delivery

`sprite_animation.camera` is an optional ordered keyframe track. Each keyframe
declares a timing frame, normalized center, zoom from 1 through 4, and the curve
to the next keyframe. REEL validates frame order and shot bounds, composites the
sprites, then applies the camera crop and scale at delivery FPS.

The camera center is clamped against the current zoom before execution and the
FFmpeg crop is bounded again at render time. This prevents a follow or whip near
the edge from revealing pixels outside the composed frame. Output geometry is
explicit: a 16:9 proof does not stand in for a required 9:16 or square proof.

Choreography compilation translates beat-bound `hold`, `follow`, `whip`, and
`settle` phrases into this track. The artifact report records
`mixed_media.sprite_camera_tracks` so camera execution remains auditable.

## Unified production package

A `reel.production-package.v0.1` descriptor is a package-relative inventory of
production components. Supported kinds include production manifests, score
plans, choreography, craft plans, department packets and receipts, render
artifact reports, videos, captions, and review evidence. Every component has an
authored SHA-256; absolute and parent-traversing paths are rejected.

```yaml
schema: reel.production-package.v0.1
work: example-film
revision: review-03
publication_scope: release-candidate
components:
  - id: manifest
    kind: production-manifest
    path: manifest.yaml
    sha256: 64-character-hash
  - id: picture
    kind: render-video
    path: picture.mp4
    sha256: 64-character-hash
  - id: editorial-review
    kind: review-evidence
    path: reviews/editorial.json
    sha256: 64-character-hash
review_gates:
  - id: editorial
    owner: editor
    status: approved
    evidence_component: editorial-review
```

Create and verify the path-free receipt with:

```powershell
cargo run -- production-package-receipt package.yaml `
  --output-path package.receipt.json --output json
cargo run -- production-package-check package.receipt.json package.yaml --output json
```

The receipt binds the descriptor hash, work, revision, publication scope,
component IDs, kinds, hashes and byte counts, and declared review gates. It
contains no filesystem paths. `required_components_verified` means only that
the declared bytes match. `review_gates_approved` requires at least one gate and
explicit `approved` status with a bound evidence component. `release_ready` is
true only when both conditions hold; REEL never infers an approval from a valid
render or receipt.
