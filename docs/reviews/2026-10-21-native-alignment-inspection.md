# Native alignment inspection: producer review

Method: producer-applied Sound Designer, Editor, Rights/Provenance and fixture/
lifecycle/repository-size checklists. No independent agent or human approval.
Scope: shared alignment validator, renderer loader reuse, selected metadata path,
opt-in clock/event inspector, tests, UI edge descriptions and documentation.

Sound/editor: native sample clocks and semantic event boundaries stay explicit;
no authored seconds or implicit resampling. Exact take identity, positive clocks,
all marker bounds and required event ordering/coverage hold invalid metadata.
Mixed rates, overflow and segmented cue handling remain explicit holds.

Rights/provenance: JSON consistency is not listening, source/word fidelity,
provider accuracy, consent, creative approval or delivery verification. Flags
for audio-content and measurement truth stay false. No original media, source
wording or credits are added to reports; inputs remain private consumer assets.

Fixture/lifecycle: seven new tests exercise native clock/spans, identity and all
marker bounds, missing/nonzero event starts, wrong descriptors/corrupt first
copy, mixed rates/overflow and explicit opt-in/budgets/snapshot changes. Existing
source-text tests validate the shared metadata-path refactor. 153 asset tests, 47 shared assembly tests, 21 renderer regressions, 13 UI DOM
checks and strict crate/renderer clippy pass locally. Full CI remains required
before integration. Browser tests use
a DOM double; independent visual acceptance remains open.

Selected source-text/alignment bytes also pass the renderer's direct native
JSON parser before generic graph projection, so duplicate struct fields and
unsupported byte-order marks cannot be silently normalized into a pass.
