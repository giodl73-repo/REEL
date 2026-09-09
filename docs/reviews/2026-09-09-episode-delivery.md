# Episode verification and scene review: technical craft review

Scope: episode consumption checker, boundary findings and controlled native scene
comparisons. These are AI checklist findings, not human creative approval.

- Editor / major: file presence cannot establish scene order or timing. Resolved
  by full decoded picture/audio comparison at ordered offsets, timestamp continuity
  checks and a one-frame cumulative rounding ceiling. Overlapping transitions
  remain outside this exact-concatenation contract.
- Sound / major: a selected effects/music file may never reach the mix. Resolved
  by D/M/E recombination and exact final PCM consumption. Boundary heuristics can
  flag a valid foreground sound or miss an equal-level ambience reset; findings
  require scoped decisions and actual listening remains separate.
- Animation / major: a receipt alone cannot establish a visible effect. External
  layers must bind a named precomposed video or explicit clean-master exclusion;
  semantic effect inclusion, onset/sustain/release and separate captions retain
  their existing checker and visual-review requirements. No broad VFX pass asserted.
- Platform / major: convenience exports cannot establish identical comparison
  inputs. A/B requires identical decoded dialogue and geometry, uses lossless
  picture sources, provides three explicit sound variants and keeps labels outside
  picture. Actual browser playback remains a consumer review, not a claimed test.
- Provenance / major: an exception can outlive its reviewed cut. Boundary decisions
  bind both scene receipt hashes; evidence, source jobs and outputs are rechecked.
  Decisions and model-generated findings grant no publication or selection.

Validation: synthetic FFmpeg coverage exercises ordered consumption, missing mix
content, timestamps, boundary decisions, changed-dialogue rejection, overwrite
refusal and output tampering. The same integration test is configured for Windows
and Linux CI. Local formatting, Clippy, tests and role checks are recorded in the
handoff once completed; remote CI is required before merge.
