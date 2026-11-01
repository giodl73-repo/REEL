# Primary PNG sequence producer review

Entry `ea51342a28f8babe6d5bc84597fb5cabb7ec51a5`; controls and acceptance
scope are in [the contract](../primary-png-sequence-overlay.md). Generic REEL
media behavior; Tig source identity, selection, guards, contexts and job ownership
remain the existing authorities. No project input or runtime is activated.

Storage/privacy lens: resolve all ordered frames through the admitted primary
catalog and retain existing byte guards. The decoder reads existing images;
only bounded control/index metadata is generated. The physical-path FFconcat
file is exclusively created, kept alive through its subprocesses, then removed
through its owned tempfile guard. Quoted paths are data; control characters,
relative locators and oversized controls refuse at admission. Published indexes
contain logical FileRefs, never provider paths. Repeated hashes do not imply
extra unique objects. Reference staging reserves a bounded control per sequence.

Identity/Git lens: empty frame arrays are omitted from older serialized jobs.
Every used frame expands through the same current primary binding logic; a
frame-only draft input refuses before media reads. Context/recipe scopes include
those consumed IDs. Order stays in the job and retained index, so deduplicating
guarded objects does not erase ordering. The template adapter code identity
changes: old frozen contexts retain their original qualified runtime; this
feature does not reattribute or rewrite their evidence.

Editor/verification lens: the first source must match selected evidence, every
frame must be a nonanimated alpha PNG at exact geometry, and the array length
must equal the conformed semantic span. Formats/bit depth remain constant.
The known black/white oracle initially caught a shared renderer/reference error:
millisecond base timestamps skipped a first frame and repeated another at 24 fps.
Both new image-overlay streams now use the exact integer delivery clock. Matching
two implementations was insufficient; the explicit source-order oracle caught
the mistake. Existing video/ASS behavior remains the regression control.

Local validation: 30 regular delivery cases pass (15 native cases explicitly
ignored in that invocation), 12 template/context cases pass (three explicit
native cases ignored), the path/control-injection unit case passes, and the
separate legacy sample-offset/stems/frames/tamper native case passes. The new
fixture verifies every selected black/white frame at 24 and 30000/1001 fps,
fractional-rate still overlays, quoted paths, repeated identities, no image-copy
or input-movie outputs, exact logical index, cleanup, primary-reference replay
and tamper/count/type/field refusals. Windows Source preparation also protects a
frame-only input through the ordinary guard set. Locked all-target/all-feature
Clippy with warnings denied, formatting and whitespace checks pass.

These are producer checklists, not independent or creative approval. Hosted
qualification, actual-project source/production correspondence, explicit input
eligibility and runtime adoption remain open.
