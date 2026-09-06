# Frame-conformed score-state renderer handoff

## Scope

REEL v0.3.21 adds an opt-in `score-state` animatic edit mode for discrete
frame-authored presentations. No private media or consumer repository content
was used or copied into REEL.

## Review record

- **Editor:** hard cuts are motivated by state changes; cinematic dissolves are
  disabled, and every state owns an explicit positive frame hold.
- **Sound designer:** checked master audio is hash-bound, then deterministically
  padded/trimmed to the picture timeline so sound remains structural without
  controlling or shortening the container tail.
- **Platform/audience:** delivery FPS, dimensions, captions, and clean-picture
  behavior remain explicit CLI/export choices. Clean picture is suitable for a
  separately disclosed share copy but does not remove accessibility planning
  from the manifest.
- **Rights/provenance:** rendering and validation are local; retained inputs are
  hashed in artifact lineage, existing path-free receipts remain available, and
  technical success does not imply selection, consent, release, or publication
  approval.

No disagreement was identified among these checklist findings. This record is
an engineering review, not actual principal or publication approval.

## Validation

- `cargo check`
- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `git diff --check`

The regression suite includes a synthetic 304-state, 50 fps timeline with one
20 ms frame per state. It verifies a 6,080 ms contract, 304 exact holds,
hard-cut assembly, a final `trim=end_frame=304`, and absence of `-shortest` in
score-state output.

## Consumer integration

Build or install this branch, author one manifest shot per score state, and run
`animatic-render` with `--edit-mode score-state --fps 50 --clean-picture`.
When binding a pre-mixed master, provide its passing `--audio-check-report`.
After rendering, run `animatic-check`, then generate and verify an animatic
receipt for a portable handoff if required.
