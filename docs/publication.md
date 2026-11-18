# Publication contracts and comic production

`reel-publication` makes film and comic editions explicit publication formats.
It owns format/edition metadata and named output-set contracts; `reel-workflow`
coordinates intelligence and production tasks; existing REEL/accepted project
adapters build the actual artifacts; Tig owns bytes and retained protection.
The crate does not upload, select, approve, certify press files or implement a
second storage publisher. Existing production manifests remain unchanged.

## Implemented

- `Publication` groups explicit language editions with stable IDs.
- Film editions reference an existing pinned composition source; the existing
  film compiler owns all timebases and timing semantics.
- Comic editions declare reading direction and an ordered page list. Each page
  names an exact source, story/cover/matter role, stable panel/utterance order and
  optional spread identity. A spread requires exactly two adjacent pages; this
  is not an assertion of printer-facing parity or actual readable geometry.
- Named `OutputContracts` and `ArtifactSet` values retain independent logical IDs,
  hashes, lengths and media types. Required outputs must be present; undeclared
  outputs, conflicting logical identities, wrong types and oversized descriptors
  refuse. Optional preview outputs are declared explicitly. SVG and structured
  JSON media types are supported.
- `reel-publication check MANIFEST` validates bounded metadata without opening
  referenced media or starting production. Its report explicitly makes no
  render, byte-verification, selection or print-readiness claim.
- The opt-in `comic` feature now supplies a bounded private page producer:
  independently pinned PNG art layers, ordered rectangular panels, editable
  balloon/text SVG, clean and reading SVG/PNG, an exact-text JSON companion,
  a lossless raster reading PDF and a complete captured source folder. Existing
  shared `reel-print` font, SVG, raster and canonical PDF machinery is reused;
  no BBJ story, compositor implementation or cache engine is copied.

Metadata intake is bounded to 1 MiB. There are at most 32 editions, 512 pages per
comic edition, 256 panel/utterance IDs per page and 256 named outputs per set.
Oversized declarations refuse; nothing is truncated. Native actor/changed-path
budgets remain separate and may admit a smaller production unit.

The workflow prototype now uses `reel.workflow-{plan,packet,result}.v2` and named
output sets from this crate. The earlier single-output v1 prototype is rejected,
not silently migrated. The task key includes every output contract. Results and
dependency descriptors include the complete declared returned artifact set.

`reel-workflow check-result PACKET RESULT PAYLOAD_BINDINGS` takes an explicit
local JSON map from output name to payload filename. It checks the exact key set
and streams every returned file through hash/length verification. These local
bindings are private machine data, separate from the portable descriptors. A
valid result remains unselected and unretained until the managed lifecycle
adapter establishes its actual retained-owner acknowledgement.

## Comic production entry to qualify

The intended streamlined entry is one project publication definition plus its
current script/design/language selections. The runner derives bounded work for:

```text
script and continuity locks -> thumbnails and balloon plan -> reference locks
-> original layered illustration candidates -> lettering -> page/spread proof
-> ordered issue reading edition -> scoped findings/fixes -> new selected edition
```

Direct and prompt-handoff execution use the same graph for intelligence tasks.
Deterministic tools consume pinned recipes separately. Reuse existing exact
artwork and recordings; no rerender/regeneration follows from opening a project.

Project definitions supply visual school/style, layout intent, panel geometry,
language text, lettering/font and physical/digital target policies. Profiles must
not turn a preferred panel-count range into a grid template or prescribe seconds
per static panel. Page count, page purpose and reading experience are different
constraints. A language edition may need different balloon geometry without
changing clean art. A changed page can require surrounding spread/issue review
even when only its own outputs rebuild.

Retain the source graph for editable layers, text, fonts, placements, masks and
source rights, plus exact selected reading derivatives. Clean-folder recovery
must prove clean art reconstruction AND a fresh lettering edit/compose, not
merely reproduce a flattened raster. Treat wrong layer ownership, clipped
balloons, unintended reading order and poor page turns as genuine findings even
when byte reconstruction passes.

## Explicit comic producer

Build the optional producer with `cargo build -p reel-publication --features comic
--locked`. The default metadata/contract build stays independent of the renderer.

```text
reel-publication comic-render REQUEST NEW_DIRECTORY
reel-publication comic-check REQUEST PACKET
reel-publication comic-restore PACKET NEW_DIRECTORY RECEIPT_SHA256
```

`reel.comic-build.v1` declares publication/edition/language, exact font and license
pins, ordered page-source pins and input/output byte allowances. Every
`reel.comic-page.v1` source declares its own canvas, ordered panel rectangles,
separate background/character/prop/effect PNG layers and exact utterance IDs,
speakers, text lines, boxes, font sizes and optional tails. Paths are portable
relative names; their actual bytes are verified before use. No implicit script
rewriting, automatic grid, font download or image generation occurs.

The producer checks declared placement/identity, panel coverage, glyph presence,
conservative advance-based line fit and balloon height. It renders all inputs
from captured bytes before creating the requested NEW output directory. Each
output file is exclusive-create; failed partial output is preserved. The packet
includes `sources/request.json` and the exact used page, art, font and license
files. `comic-check` recomputes the expected packet, rejects extra/missing files
and compares all bytes. It is an explicit render/rebuild check, unlike the
read-only publication metadata `check` command.

Restoration requires the externally pinned receipt SHA256. It verifies every
declared source before creating a new destination, then restores the original
relative source layout. That independent source folder supports a canonical
rebuild AND new lettering versions without the old working source directory.
This is a development packet, not a new asset store or a substitute for Tig
record-to-byte binding and retained-owner acknowledgement.

Current worker bounds are 32 pages, 32 rectangular panels and 128 layers or
utterances per page, canvas/PNG dimensions at most 2048 each, 64 MiB captured
inputs and 128 MiB complete output packet. Expanded repeated layer bytes are
limited by both input allowance and one quarter of output allowance; expanded
pixels are bounded separately. Smaller request allowances still apply. The
reading PDF contains lossless full-page RGB images; editable/searchable text is
retained in the SVG/JSON sources. It is not a tagged/accessibility-certified or
press PDF. Larger print-resolution pages, arbitrary panel polygons, vector
effects, vertical-scroll formats and richer typography need additional adapters
and fixtures rather than silently ignored fields.

The synthetic two-page Spanish/English fixture passes actual Rust rendering and
canonical checking, a one-line Spanish fix with protected art/page/English bytes,
original-source unavailability, pinned source restoration and a fresh text edit.
Five written two-page PDFs were independently rasterized with installed PyMuPDF;
all ten RGB page rasters match their selected reading PNGs exactly. All six
distinct page appearances were visually inspected for lettering, accents,
balloons, borders and margins. This is producer-applied technical proof over
simple synthetic shapes, not finished illustration or creative approval.

## Remaining implementation and acceptance

The default crate validates declared structure; the optional producer implements
the bounded operations above. It does not build CBZ/OpenRaster archives, infer
multilingual equivalence, prove semantic art ownership or qualify a printer.
Existing `reel-print` collection behavior remains separate and its regression
fixtures continue to apply.

The workflow binding now checks a pinned producer against the selected edition,
complete source contract and returned receipt; see
[deterministic publication task binding](workflow-orchestration.md#deterministic-publication-task-binding).
The local checker executes no tool and makes no native snapshot/custody claim.
The native workflow now connects comic receipts, deterministic replay, candidate
publication and retained proof to journal completion. See the
[native comic handoff](handoff/2026-11-18-workflow-native-comic.md).
Broader production should generalize accepted
compose/assemble/verify/rebuild/pdf behavior with synthetic fixtures and original
upstream source rights. Do not change BBJ's admitted tool versions, port its story data, execute
frozen authoring helpers or claim that its current LFS/native profile switched to
git-asset. Remaining layer-package adapters require their own explicit port and
restoration tests. A successful old project tool invocation is not qualification
of a new shared adapter.

The first rendering/source-restoration fixture is implemented. Stale workflow
returns have separate contract tests. Bounded synthetic native handoff now covers
actual protection, logical registration, deterministic replay and retained native
outputs. Larger issue planning, expected-baseline selection and project adoption
remain open under the upstream lifecycle. Only after
that qualification and owned rollback should a project adopt the shared producer.
Issue-level review/selection, printer preflight and delivery remain distinct gates.
