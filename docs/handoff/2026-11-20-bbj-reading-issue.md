# Actual BBJ selected reading issue

The new `comic-reading-assembly` capability assembles a complete selected reading
edition from existing opaque PNGs. BBJ's actual 28-sheet book now produces
`F:/src/tig-dev-temp/reel-bbj-reading-issue-20261120-v1/reading/reading.pdf`.
It is a private local derivative, not a retained native completion, an editable
archive, printer proof, new creative approval or selected publication.

## Actual evidence

The root `F:/src/tig-dev-temp/reel-bbj-reading-issue-20261120-v1` contains:

- `config.json` and `capsule/`: exact runtime, selected book, complete intake,
  captured sources, manual return and late-bound native recipe.
- `planning-result.json` and `composition-result.json`: successful Rust CLI
  preparation and complete local reading assembly.
- `reading/`: PDF, reading manifest, source index and exact worker-style receipt.
- `pdfinfo.txt`: 28 pages, PDF 1.7, unencrypted, 114,110,367 bytes.
- `ordered-png-paths.json` and `pixel-validation.log`: read-only Rust validation
  of every embedded image's dimensions and RGB pixels against the exact ordered
  selected PNGs. All 28 pass; alpha must be opaque. No raster resampling occurs.
- `existing-output-refusal.json`: `Conflict/bundle-create-new`, before replay.

PDF SHA256:
`066806dae490c3bab41307cbfa1f1609f455446431787be35f637d867b10937f`.
The selected-book hash remains
`c9d0b5b412643e26790f170c46abe2dbcf19dd4102f1b7249c2323331a776d94`.
The captured intake has 88 files / 127,066,343 bytes, including the native
runtime; reading PNGs account for 100,401,060 bytes. There were no provider calls,
changes to BBJ/Melena, native admissions, selection changes or scratch retirement.

Private preview v4 is at
`F:/src/REEL-releases/workflow-preview-20261120-v4/START-HERE.md`.
Both optimized binaries identify source commit
`a347b1d` in its manifest. The core binary SHA256 is
`dd666ce6019928bad1510a9e407f93fa9ec33c7a7595466b1d9eae28e356a980`;
the native binary SHA256 is
`a62c825756d1be63374d74627855a275c53ec49c628889e960f26aa8d0181070`.
Installed hashes and command availability are checked; the packaged synthetic
planning example passes. This revision passed 155 core workflow tests, 103
standard native tests and all 247 SDK source pins. Strict Clippy passed for core
comic/default and native film/default. The print suite and explicit actual-book
pixel check pass. Opt-in native production tests were not run as part of the
standard suite and are not included in those counts.

## Contracts and checks

Source, authoring and PNG associations must match every selected sheet exactly.
Publication page order, authoring panel/dialogue order, logical input identities,
language and complete source coverage are checked before generation. Page,
panel and dialogue review targets use the original stable IDs. The review
manifest references original input PNG artifacts: native review admission must
carry their original retained lineage, rather than assume they are outputs.

The workflow tests cover deterministic replay and producer-bound review-control
generation, including original image references. Honest-pinned missing pages,
wrong selected associations and changed panel order each refuse with their
specific semantic conflict. Shared print tests verify lossless embedded pixels,
ordered page geometry, alpha refusal and byte limits. Existing layered tests
continue to pass.

The reading request explicitly permits 256 MiB input/output; the intake remains
bounded to 120 files / 128 MiB. Individual PNGs are bounded to 16 MiB and a finite
pixel canvas. Native return, worker reservation, actor/path and metadata capacity
must be checked separately before accepting this recipe. No admission limits
have been relaxed in the qualified SDK. The 64 MiB older comic path is not a
universal film limit.

## Producer checklist findings

Applied `.roles/comics-editor.md`, story direction, editing, platform/audience
and rights/provenance as technical producer checklists, not reviewer identities.
Complete page coverage and image fidelity pass. This reading derivative retains
the selected visible lettering, with no inferred translation or reflow. The
source index explicitly says editable layer payloads are not restored. Display
pages fit proportionally inside 720 points; this is not physical trim or printer
qualification. Accessible/searchable text and visual readability inspection
remain separate. No human review, rights decision, creative approval or release
is claimed.

## Remaining acceptance work

Run the full reading recipe through bounded native retention and review, then
exercise an actual issue, scoped fix and affected consumer rebuild. Qualify the
complete editable source archive independently. S5 resource-scale production
and owned profile adoption/rollback remain open; this PDF does not close them.
