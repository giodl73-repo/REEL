# Timed still overlay producer review

Entry `03a21cef64d6b277adeb7fb2e86154eb039ecd36`; scope and L0/L1/L2
controls are in [the contract](../timed-still-overlay.md). This adds a generic
primary-image composition mode to the existing renderer and primary-reference
pipeline; it does not enroll project inputs or change production selection.

Editor lens: use the existing half-open semantic frame interval and frame
conformance, including overlays that cross a picture cut. Layer order remains
explicit. Source image rate is not an editorial clock: loop a single PNG at the
declared delivery rate, without weakening timed video's frame-rate/sufficiency
rules. Checker visibility and adjacent outside-span checks remain in force.

Rights/provenance lens: the selected image is consumed directly. Alternate
render-source/derivation receipts and fonts refuse in this mode. It carries no
creative or public-release approval. Reject grayscale/RGB images without alpha
and require a supported PNG alpha pixel format, rather than treating the letter
`a` in a pixel-format name as sufficient evidence of alpha support.

The animation refusal fixture found another platform pitfall: the same two-frame
APNG is reported as `apng`/two frames at a normal Windows path, but as `png`/one
frame through an extended canonical path on the exercised FFmpeg build. Direct
bounded PNG-content inspection therefore rejects animation before relying on
demuxer metadata. The fixture verifies it generated a genuine two-frame APNG;
two identical frames can instead be optimized into a valid single PNG.

Storage/recovery lens: the shared output contract includes the exact staged PNG;
managed output admission/retention continues to own its budget and lifecycle.
No intermediate video is an authored dependency. Existing primary-reference
process/staging admission includes the new layer; replay remains bounded and
writes no comparison movie. This low-level API still requires the caller's
existing source/runtime protection, resource admission and process containment.

Compatibility: old modes retain their output names and command options. The
new mode is explicit; no PNG is silently treated as a video overlay or selected
because it happens to be available in a cache. Native implementation and runtime
qualification, actual-project correspondence, selection and adoption remain
separate. These lenses are producer checklists, not independent approval.

Local validation: all 29 regular scene-delivery cases pass (15 native cases are
explicitly ignored by that invocation). The new case covers single/two-layer
rendering, exact source/output pins, cut-crossing spans, native checking,
independent all-frame/PCM primary-reference replay, tamper and malformed-source
refusal, including the genuine APNG. The separate native legacy sample-offset,
stems, frames and tamper case also passes. All-target/all-feature locked Clippy
with warnings denied, formatting and whitespace checks pass. Hosted qualification
and actual private production correspondence remain open; no runtime is installed.
