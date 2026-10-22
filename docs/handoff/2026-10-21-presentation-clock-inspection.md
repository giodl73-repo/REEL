# Selected presentation template and clocks

Explicit immutable scene-context `presentation_template` descriptors opt into
shared in-memory editable-layer compilation. Source and alignment checks must
also be opted in and valid for the checked language. Template identity/content,
soundtrack requirements, source/display coverage and native markers use the same
pure Rust helpers as the renderer. Existing chapter-overlay clock unit tests
moved with their implementation to reel-assembly.

Reports include layer and scene clocks plus ASS hash/bytes, without emitting
ASS/text. Per-language failures hold the context; native/source/alignment/media
presence flags remain separate. There is no media rendering, audio decoding,
font/layout acceptance, archive access, hydration, mutation, service or approval.
Metadata/work budgets and snapshot checks remain explicit. Private fixture
failures belong to the producer's exact-version evidence and backlog; do not
rewrite another lane's scene, source or Golden to manufacture a green report.

See [contract](../asset-references-v1.md) and
[producer review](../reviews/2026-10-21-presentation-clock-inspection.md).
Next: close the private authoring coverage hold in its producing lane, then
separate font/layout/render and measured-audio acceptance when explicitly run.
