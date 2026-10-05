# Motioncraft scene-engine checkpoint — simulated role review

Scope: current implementation on `codex/motioncraft`; five core production
checklists plus rights/provenance. Review is by the current assistant, not
independent agents or real people. This is a checkpoint review, not goal
acceptance or creative approval.

| Lens | Finding | Disposition/evidence |
|---|---|---|
| Story | Unsupported shape/type effects could be mistaken for execution. | Current camera renderer rejects noncamera element roles. Documentation identifies the bounded treatment and outstanding visual-direction work. |
| Animation | Continuous phase endpoints, fractional sampling and reduced motion must reach real pixels. | Test renders patterned imagery, verifies a stationary hold and changed push endpoint, then proves reduced motion stays still. Scene renderer consumes the compiled plan. |
| Editor | Local rounding can differ from cumulative picture allocation. | Explicit allocation accepts at most one frame of boundary difference while preserving sample spans; scene plan reconstructs and validates it. |
| Sound | Camera upgrade must not change dialogue/effect timing. | Two-cue test retains 48,001/47,999 samples and effect offset 123; baseline/directed/reduced tests compare exact dialogue and effect bytes. |
| Platform | Indexed evidence can be mislabeled by timestamp seeking or unbounded extraction. | Frame-counter selection, PNG-to-picture verification and independent full-decode comparison; frame/raw-pixel/expression limits checked before rendering. Landscape/portrait matched boards retain text. Declared protected regions are checked through the camera transform. |
| Rights/provenance | Re-reading files for receipt hashes can bind evidence to different bytes than those parsed. | Compiler uses parsed input snapshots, including the alignment manifest; job plan hashes its parsed buffer. No CAIMITOS media is used. |

## Open completion findings

- Explicit native-duration fitting is implemented and tested with a reusable
  48-frame default resolving to a 24-frame selected shot. Native samples remain
  unchanged. Delivery-profile safe areas and transformed protected content are
  implemented; these are declarations, not automatic content detection.
- Typed typography/palette intent and six retained landscape/portrait renders
  are available. Font/art generation is an owner-side responsibility, not a
  renderer capability implied by this metadata.
- Matched boards have been visually inspected at frames 44, 149, 269 and 389.
  Revised typography is slightly larger without losing text. Baseline and
  reduced variants retain the same diagram and claims. No viewer comprehension
  or playback-based score is inferred from these stills.
- Caption review found overlap with the landscape lower label in the first
  derivative. A smaller caption size and adjusted margin fix the observed
  overlap in `review-comparison-v2/caption-check.png` for both profiles. The
  first derivative is retained; v2 is the current candidate.
- Full playback evaluation and scored visual review, hydrated package, and
  actual CAIMITOS adapter new/existing episode adoption remain required.
  Synthetic scene-engine success does not close those gates.

Retained disagreement: expressive camera movement can aid rhythm but can also
harm reading. The original/reduced treatment and authored holds remain explicit;
the queue comparison must evaluate the effect rather than assume movement wins.
