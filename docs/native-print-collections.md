# Private native print collections

`reel-print render REQUEST NEW_DIRECTORY` and `reel-print check REQUEST DIRECTORY`
provide shared Rust print production. Build with `--features print --bin reel-print`.
The optional `reel-print` crate is separate from music interpretation and storage.
No Python, browser, system font search, provider, upload, Git mutation or cache
initialization occurs. A Tig project view or managed job supplies named files;
the renderer consumes their exact immutable pins rather than discovering assets.

`reel-print plan REQUEST` computes the exact packet in memory and reports total
bytes/files, including the receipt, before any output directory exists.

The JSON request is `reel.print-collection.v1`: title, author, notice, explicit
`private_only:true`, captured font file pins, `body_font`, ordered entries and
`limits:{input_bytes,output_bytes,pages}`. Every source/font pin has `path`,
`sha256`, `bytes`. Relative inputs stay beneath the request directory; explicit
absolute inputs are supported. Each file is captured and verified once before
parsing. Rendering uses snapshots, so subsequent source writes cannot change
the consumed bytes. Inputs are limited to 32 MiB each, 256 MiB total; requests
stay within 64 KiB and collections within 256 entries and 1,024 pages. A 52-song
book can therefore keep separate text/notation sections within an explicit budget.

Entry kinds:

- `text`: stable `id`, `title`, pinned UTF-8 `source`; canonical wording stays
  unchanged while long lines/pages wrap using the captured font's metrics.
- `parallel-text`: `id`, `title`, pinned `left`/`right` text, `left_label` and
  `right_label`; ordinal source lines remain paired across wrapping and repeated
  column headers. This does not infer translation alignment or word timing.
- `native-score`: `id`, `title`, pinned SVG `source`, `systems_per_page` (1..4).
  The supported REEL native grid is 1100 units wide, with five staff lines at
  10-unit intervals and systems 150 units apart. Short pitch ledger lines remain
  untouched and do not count as extra staves. Exact contiguous source bands are
  recorded. Presentation scales/clips those bands without rewriting notation.
- `retained-pdf`: `id`, pinned PDF `source`. Page order and decoded content
  streams remain unchanged; inherited resources/geometry are materialized before
  assigning the new page tree. Existing files remain immutable. This is retained
  page assembly, not recovered editable score authority.

Captured fonts are the only available font database. Missing glyphs, unavailable
families and embedding/subsetting restrictions refuse. Native SVG may not use
external/active content, images, imports or transformed staff grids. Encrypted or
active PDFs refuse. No machine check grants font distribution or musical rights.

The packet contains `collection.pdf`, source page SVG/PNG previews and a private
`receipt.json` with request/input/output pins, page dimensions and source bands.
The complete output allowance is checked before reserving a new absolute ordinary
directory. `create_dir` and `create_new` prevent concurrent overwrite. A later IO
failure leaves owned incomplete staging with no complete receipt; it never edits
an existing directory. There is no global cache or render lock.

`check` rebuilds the packet in memory, compares every byte and refuses extra,
missing, corrupt or linked files. SVG previews are not an independent rendering
of the written PDF; the receipt explicitly requires separate PDF raster QA.
Use a pinned independent renderer before accepting page appearance. Machine
checks and producer visual inspection remain separate from human musical,
engraving, selection or delivery approval. CLI success emits only counts;
expanded paths, texts and hashes stay in private packet files.

PDF serialization canonicalizes the reachable object graph in sorted dictionary
key and array order. The SVG library writes auxiliary font objects through a
randomized map; two-font documents otherwise acquire different object IDs across
processes even when their page appearance is identical. Canonical numbering
prevents these incidental byte revisions without changing font or drawing streams.
Fixtures compare independently numbered retained PDFs and run separate CLI
processes with two captured font families. Deterministic bytes support content
addressed storage; they do not imply musical or visual approval.

The fixtures cover accents, bilingual layout, long-poem continuation, nine
systems across three notation pages, ledger lines, page order, inherited PDF
resources, decoded stream preservation, deterministic rebuild, output tamper,
no-clobber, privacy, stale pins, duplicate IDs, budgets and active SVG refusal.
Collection listening HTML, passage timing, calibrated piano, new engraving and
editable score changes retain their own adapters and acceptance requirements.
