# Native Tig backend review

Scope: optional committed-source adapter, source pin and explicit-profile CLI.
Producer-applied editor, sound-design and rights/provenance checklists only;
not independent review or cutover approval. Tig donor source is
12757c21e9f896946b21a6a0dca653bf61177e5b, MIT; 45 runtime-source/manifests/license
files, 480,962 raw bytes. Upstream source remains unmodified and hash checked.

- Editor: exact logical IDs and expected pins are supplied by the job; changes
  require an explicit Git view/binding update. Historical replay restores the
  exact job bytes. There is no hidden latest or language fallback.
- Sound designer: the named-stream selector is explicit per admitted definition;
  cue timing and original samples flow into existing REEL compilers/renderers.
  Transport and decoding do not infer production or listening acceptance.
- Rights/provenance: source and definition pins are exact, payloads use existing
  Tig readers, guards survive catalog drop, and partial preparation releases them.
  No private asset/prompt is imported into REEL; cache paths stay out of result
  JSON. Profiles and operational diagnostics remain private.

Corrected finding: passing a relative bare job filename directly into the legacy
renderer supplied an empty parent path on Windows. The new CLI resolves its job
to an absolute file, and the end-to-end test now exercises relative job arguments.

Dependency finding: private-repository CI cannot rely on a sibling checkout or
an unspecified cross-repository token. The optional adapter pins unchanged Tig
source under vendor, with its MIT notice and CI raw-byte checks. This is an
explicit dependency snapshot; fixes/upgrades go upstream, not into a local fork.
The feature requires Rust 1.89+; ordinary builds remain independent of Tig.

Scope limits: path-based protection is currently Windows only; cooperative mode
is bounded read/seek access and never substitutes for guarded paths. Definition
admission does not execute all schema/rule semantics. Full primary closure,
producer freshness, durable job recovery, custody, eviction and native CAMINOS
cutover are OPEN. The result always reports incomplete semantic coverage.

Local focused tests and the real standard CLI fixture pass. Final full-workspace,
Clippy/default-build checks and exact-head Windows/Linux CI must be recorded
before merge. Disposition: pass with these documented scope limits.

Final local record: full all-feature workspace 976 passed/37 ignored. The final
backend target passed 12/one ignored after deduplication and receipt-bound fixture
updates, followed by an explicit passing real CLI render/version/replay run.
Workspace all-target/all-feature warnings-denied Clippy, no-default-feature library
compilation, formatting and whitespace checks pass. Exact-head Windows/Linux CI
is still a merge obligation; full readiness requirements remain OPEN.

Actual-fixture finding corrected: asset IDs/definition IDs are bounded opaque
tokens, while Git scope/placement paths have separate portable-path constraints.
The source's colon-separated IDs now resolve unchanged. A regression captures
opaque IDs through real Git. Font output naming was also isolated from opaque
IDs: valid legacy basenames remain unchanged; unsafe names use the exact byte pin.
The real mapped-provider test now includes an opaque ASS/font ID and preserves
decoded pixels/PCM. Final targeted count: 13 passed/one ignored, with the real
native CLI case separately passed; warnings-denied Clippy remains required again.
