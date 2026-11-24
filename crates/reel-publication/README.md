# Comic publication commands

Build the Rust publication CLI from REEL:

```powershell
cargo build --manifest-path crates/reel-publication/Cargo.toml --features comic --locked --bin reel-publication
```

The existing `comic-render`, `comic-check`, `comic-restore` and
`comic-issue-assemble` commands handle REEL's page/issue contracts. Use the
following adapter for BBJ's existing ordered SVG layer sources.

## Prepare and compose a BBJ book

Set `$selection` to the exact selected `refraction.book-preview.v1` JSON. Use a
new output directory for each command. `my-issue` is your stable publication ID;
the language is an explicit edition declaration, not a translation operation.
The output parent directory must already exist.

```powershell
reel-publication bbj-prepare $selection F:/src/bbj my-issue en F:/work/my-issue-prepared
reel-publication layered-compose-book F:/work/my-issue-prepared F:/work/my-issue-review
```

Open `F:/work/my-issue-review/index.html`. The local reading copy labels every
sheet with a stable page ID and carries the original panel reading IDs where
present. It uses the selected reading PNGs unchanged. Every PNG is hash-checked,
and its page SVG must first reproduce the selected SVG exactly. No image model,
network service, font substitution or new raster rendering is involved.

The prepared directory contains an ordered `book.json`, exact selected book
control, reading bindings and a request/bindings pair for every sheet. Each
request preserves the selected layer paths across reused drawing folders and
pins its authoring record, original script and declared font provenance.
Story IDs use the publication and story-page number; book-matter IDs use its
declared kind. Drawing-version directory names are not story IDs.

The review directory contains the reader, per-page SVG/PNG, exact layer source
bytes, listed provenance, per-page source receipts, completion checkpoints and a
book receipt. Page composition is sequential. Declared input allowance is bound
per page (maximum 256 MiB); the local book output estimate must fit 8 GiB. These
are this adapter's bounds and do not alter legacy comic or film limits.

The actual selected Counterphase 2 book has now passed this path: 28 sheets,
28 exact SVG reproductions and 28 unchanged reading PNGs, with a complete local
reader and 529 output/source files (1,400,769,540 bytes). The frozen local producer
at source `0a419a5d565758f2f9dc833f4a764dc16a5328e2` completed in 249.99 seconds.
The private result is
`F:/src/tig-dev-temp/reel-bbj-book-20261120-v1/book/index.html`; its parent evidence
directory records the frozen tool, actual run, reader checks and validation.
`tool.json` is the compiled-source authority; `source-provenance.json` clarifies
that the run wrapper observed checkout HEAD again at completion. The book
receipt SHA256 is
`825c66dd828277f8b90bdf4aa8789f57e56c97ac01cae52a2645502506c51022`.
This is actual selected-source compatibility and local publication evidence,
not a creative viewing, new raster rendering or native custody qualification.

To compose one request independently:

```powershell
reel-publication layered-compose F:/work/my-issue-prepared/sheet-001/request.json F:/work/my-issue-prepared/sheet-001/bindings.json F:/work/my-page-candidate
```

This produces an SVG and preserved source bundle. It does not produce new PNGs.
Layer order, offsets, opacity, visibility, original text and SVG content are
preserved. Invisible source layers are also retained. Unsupported layer geometry,
external references, active SVG, changed hashes and source path aliases refuse.

## Current boundaries

These commands operate on private local files. They do not adopt a BBJ profile,
admit a Tig job, acknowledge retained ownership, select candidates, authenticate
creative approval or release a book. A failed/interrupted run keeps its intent,
checkpoints and partial files; automatic book resume is not implemented yet.
Use the native workflow lifecycle separately for admitted jobs and retain their
original identities/runtimes.

The captured sources cover the selected SVG layer stack and listed provenance.
They do not establish a full original-generation archive, font license proof,
fresh PNG/lettering-edit restoration or press readiness. Rendering new reading
pixels, actual managed-job qualification, the review/fix
loop and full project adoption remain required. Keep the selected BBJ inputs and
approvals immutable while producing candidate versions.

## Verify a one-page correction

Run `check-book-delta BASELINE BASELINE_SHA256 CANDIDATE CANDIDATE_SHA256 PAGE_ID`
with the two private reader directories and their independently pinned
`receipt.json` hashes. It rechecks each sheet's SVG, reading PNG and source
receipt, verifies stable page order, and requires only the designated page's
SVG and reading pixels to change. Use the stable page ID shown in the reader.
