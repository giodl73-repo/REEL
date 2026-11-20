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
pixels, wiring this adapter into the managed workflow task graph, the review/fix
loop and full project adoption remain required. Keep the selected BBJ inputs and
approvals immutable while producing candidate versions.
