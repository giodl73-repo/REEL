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
| Platform | Indexed evidence can be mislabeled by timestamp seeking or unbounded extraction. | Frame-counter selection, PNG-to-picture verification and independent full-decode comparison; frame/raw-pixel/expression limits checked before rendering. Phone/profile and protected-region proof remain open. |
| Rights/provenance | Re-reading files for receipt hashes can bind evidence to different bytes than those parsed. | Compiler uses parsed input snapshots, including the alignment manifest; job plan hashes its parsed buffer. No CAIMITOS media is used. |

## Open completion findings

- Exact-duration defaults need explicit reusable behavior for varying shot
  lengths; no implicit timing change is allowed.
- Declared bounds currently validate author geometry only. Delivery-profile
  safe regions and transformed protected content need the planned implementation.
- Typography/palette intent, retained queue comparison, scored visual review,
  hydrated package, and actual CAIMITOS adapter new/existing episode adoption
  remain required. Synthetic scene-engine success does not close those gates.

Retained disagreement: expressive camera movement can aid rhythm but can also
harm reading. The original/reduced treatment and authored holds remain explicit;
the queue comparison must evaluate the effect rather than assume movement wins.
