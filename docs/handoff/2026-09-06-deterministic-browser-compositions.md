# Deterministic browser-composition handoff

## Scope and findings

REEL v0.3.22 adds a local Chromium adapter for frame-indexed HTML/SVG/CSS/JS
compositions. It follows the v0.3.21 score-state duration contract and does not
contain consumer media.

- **Editor:** frame index, FPS, frame count, and hard frame boundaries are the
  authoritative edit clock; wall time and cinematic transition grammar do not
  drive score-state changes.
- **Animation director:** one persistent DOM eliminates separate browser-launch
  rasterization differences. Declared dynamic-region masking makes unexpected
  stable-picture changes a blocking error.
- **Sound designer:** optional master audio is hash-bound and conformed with
  deterministic pad/trim to the exact picture duration.
- **Platform/audience:** dimensions, FPS, clean-picture behavior, and delivery
  verification are explicit. Caption/accessibility strategy remains the
  producing project's responsibility.
- **Rights/provenance:** inputs and output are hashed, browser identity is
  recorded, remote protocols are blocked, and technical verification remains
  separate from selection, rights, or publication approval.

No checklist disagreement was identified. These are simulated engineering
findings, not human approval.

## Validation

- Unit coverage for path confinement, dynamic-region background stability, and
  frame lineage.
- External integration coverage renders a two-state inline SVG score through
  Microsoft Edge, proves the highlight pixels change while the masked
  background hash stays identical, encodes exactly two CFR frames, and rechecks
  the artifact.
- `cargo fmt --check`
- `cargo check`
- `cargo test --no-fail-fast`
- repository role checker and `git diff --check`
