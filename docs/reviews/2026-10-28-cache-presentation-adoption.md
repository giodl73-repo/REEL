# Borrowed presentation sources and outputs

Opening and credits adoption previously required source/control files under local
roots even when a Tig reader already held the exact bytes. `presentation_adopt`
build, check and verification reuse now accept the existing `InputFiles` port.
`Path` and `PathBuf` preserve their checked local-root behavior. Providers retain
their guards through the synchronous operation; returned locations are never
written into the manifest or receipt. There is no hydration or source staging.

All presentation probes, conversions, decoded comparisons and timestamp checks
use the source provider's media programs. Explicit tool refusals propagate.
Verification reuse hashes those same selected executables, current controls,
source and selected output; it cannot reuse evidence when a source is withdrawn.
Local callers retain their existing ambient tool resolution.

Episode conform exposes `build_with_presentation_files` for adopted presentations
and `build_with_segment_files` for the combined native scene/presentation case.
Each presentation borrows source/control and selected-output providers separately.
The presentation map must exactly cover the episode's adopted presentation units;
the scene maps retain exact native coverage. Selected outputs use their receipt
names `master.mkv` and `receipt.json`, with unchanged hash and length checks.
Unbound files do not fall back to the local asset root. Presentation checks remain
full independent adoption checks; this injected conform path does not silently
write a verification cache in an asset root. Overall episode control JSON remains
an explicitly bounded local compilation product. Episode-level tool injection and
the full native episode job executor are separate remaining work.

## Fixture and role review

The tiny native opening fixture supplies content-addressed aliases, removes the
original source and adopted outputs, and consumes them in a real episode conform.
It then removes the presentation control originals and rebuilds/rechecks adoption.
On Windows with `tig`, the fixture holds actual Tig `PreventMutation` readers and
checks that writing the protected source fails. Other configurations exercise the
same provider boundary with hash-checked local fixture files, without claiming
mandatory mutation protection. No actual project cache is involved.

Refusal cases cover unbound source and media tool while local originals still
exist, unbound selected output despite the matching local output, missing provider
coverage, and withdrawn source after successful verification reuse. The shared
tool-command test also rejects an explicit nonexistent tool without PATH retry.
Existing excerpt, timing-fault, audio-repair, source-evidence and retimed-master
tests remain required; passing technical checks do not authorize delivery.

Review lenses applied from `.roles`: Editor requires unchanged selected frames,
ordering and duration; Sound Designer requires unchanged PCM and the existing
evidenced clock-repair rules; Rights and Provenance Steward requires exact source
selection and evidence, no network operation, no approval inference, and no cache
locations in portable receipts. The changes preserve those boundaries. These are
checklist findings, not human creative or release approval.

Local qualification: all thirteen presentation integration tests pass (99.59s),
including the borrowed-cache assembly case; both generated episode lifecycle
fixtures pass (90.17s), including the combined segment API; library checks pass
205 tests with nine explicit external cases ignored (1.88s). The final targeted
presentation run passes (38.25s) and warnings-denied all-target/all-feature Clippy
passes (49.69s), covering the later verification-count and strict-tool-selection
corrections. Formatting and diff checks pass. Hosted qualification remains required
before merge. No existing runtime binding or actual project cache was changed.
