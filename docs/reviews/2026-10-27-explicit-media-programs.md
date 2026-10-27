# Explicit media-program and declared-runtime contract

Entry REEL 3e2103713ab317714dde9bfe68da3864829662b0 (PR192, qualification
pending). Continues WP-010/011/012, REQ-006/009/010, SPEC-010/011/012,
IF-005/010/011, PKG-004/006. Scope: generic per-operation media program port,
held/hash-verified explicit runtime bindings, optional native producer use and
synthetic tests. No runtime install, PATH mutation, daemon/cache initialization,
real media job, transfer, selection or retirement.

Extend the existing trusted InputFiles port with explicit FFmpeg/FFprobe selection;
historical default callers preserve ambient behavior. Plumb every native scene
render/check/audio helper, including overlays, PCM rails and Motioncraft evidence,
through that same per-operation provider. No process-global executable selection,
thread-local shortcut or silent fallback from an explicit refusal.
Streamed PCM decoder errors must reap the child before input/tool leases end;
bounded concurrent stderr draining prevents a full diagnostic pipe blocking stdout.

A separate declared-runtime binding verifies the actual current host executable,
explicit FFmpeg/FFprobe files and named extra dependency files under caller pins.
Hold effective Windows mutation/delete-denying file guards throughout execution;
unsupported protection refuses. A portable bounded manifest contains hashes/sizes
and logical dependency names, no machine paths. Verified bindings generate that
manifest; arbitrary caller JSON cannot construct the guarded capability. Preserve
the private local binding/profile separately from the portable manifest.

The native verified-program producer binds this generated manifest Object as its
operation producer and uses only its protected absolute tool paths for render and
independent checks. Caller owns manifest/code retention and prior job/resource
admission. This establishes declared host/tool/component byte identity, not a
claim that all OS libraries, CPU settings, fonts, environment or hidden tool
dependencies were discovered. Full runtime closure and custody remain open.

L0: default/Tig-feature isolation, fmt, warnings-denied Clippy, vendor/diff checks.
L1: wrong host/tool hashes/sizes, invalid names/paths, unsupported guards and
provider refusals fail without fallback; manifests contain no local paths and
guard lifetimes remain scoped. L2: real tiny FFmpeg render/check through absolute
programs with candidate claim producer equal to verified manifest; existing
scene/audio/overlay/camera/Motioncraft regressions retain behavior on both OSes.
Exact-head qualification and parent merges precede merge.

Rights/Provenance and Editor producer lenses protect execution identity, private
paths, byte integrity, output binding and unchanged native media behavior. They
are not independent reviewers or approval authority. Full environment closure,
resource/job enrollment, retained publication/custody and adoption stay open.

## Local producer findings

The explicit tiny Windows native fixture passes with the real running host,
FFmpeg and FFprobe byte-verified under held handles. Its candidate recipe producer
is exactly the generated manifest Object, and its resulting claim survives original
context replay with Git/working metadata offline. Existing candidate tamper cases
continue to pass. The generic mapped-input fixture now combines phased camera,
ASS/font overlays, independent checks and audio-only execution through explicit
absolute program paths; every delivered picture plane and PCM sample matches its
baseline, and Motioncraft evidence is produced and checked.

The new runtime guard test passes wrong host/hash/size, invalid path/name/budget,
file/parent rename and mutation refusal, cleanup after partial binding failure,
manifest path redaction and successful independent concurrent file writes. Linux
has an explicit unsupported-mandatory-protection case; no Windows result implies
Linux path protection. Scene/audio/Motioncraft provider-refusal tests perform no
fallback, even with ambient FFmpeg available. Selected normal Windows suites pass:
27 Tig consumer, 26 scene-delivery and one runtime guard test. The broad root
all-target/all-feature suite also passes, including complete episode reuse and
imported-scene consumption. A subsequent streamed-decoder cleanup fix passes its
real child-process failure/flood test and both real native/mapped media cases;
final warnings-denied Clippy, default-library isolation, fmt/vendor/diff pass.
Exact-head cloud qualification remains required before merge.

The runtime test additionally executes a protected executable stored under an
opaque extensionless cache name; no new filename view or production copy is
required. Shell batch paths refuse. Decoder diagnostics are concurrently drained
with at most 64KiB retained, and an injected consumer failure kills and reaps its
actual child before leases end. This fixes the inspected streamed-PCM early-error
path that could otherwise outlive its input guard. PR191/192 are now qualified
and merged; their resulting main tree is 91cf06070217960f6bdc02414e3979c2eac807d1.

Rights/Provenance finding: never equate declared file verification with complete
environment closure. The generated manifest fixes complete_environment=false,
omits private machine paths and cannot be deserialized into held bindings.
Editor finding: Motioncraft evidence and float-to-PCM checks must use the same
program provider as rendering; those paths are now threaded explicitly and the
real media parity fixture covers them. Producer disposition pass_with_risk for
declared runtime files only; full environment, publication/custody and adoption
remain integration obligations, not independent or human acceptance.
