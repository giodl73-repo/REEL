# Comic issue parts and private assembly

Implemented `reel-publication::comic_issue` behind the existing `comic` feature.
An explicit plan embeds the original publication and language edition, grouping
all pages in contiguous authored parts of up to 32 pages. Missing, repeated or
reordered pages, case-colliding part IDs and split spreads refuse. A Rust API
derives checked part publication definitions while preserving original source
identity. It does not commit, select or admit them as native tasks.

The private assembler pins the plan and exact part receipts. Every part packet
replays from its own captured editable sources; exact file inventory, returned
bytes, edition/language, page source hashes, panel/utterance order and common
font/license binding must pass. Shared print code checks passive PDFs and merges
their pages in declared order without an inserted cover. The resulting PDF and
transcript satisfy the selected edition's named output contracts. Complete
editable part packets, the plan and both original/relocated requests survive in
the new output folder. Existing outputs refuse; a destination inside any input
packet refuses. Partial writes remain available for diagnosis.

The fixture uses three synthetic Spanish pages and two parts, preserving a
two-page spread and right-to-left declared order. Every assembled PDF image's
decoded RGB matches its selected reading PNG. Both original source and worker
packet locations are removed before successful CLI reassembly and pinned source
restoration. Mixed language, changed source hashes and modified lettering refuse
before an output directory is reserved. A separate 40-page metadata case accepts
32+8 pages and rejects a 33-page group; it does not render a large actual issue.

Local validation: 50 shared workflow/publication/print tests and 34 default native
tests pass. The six opt-in existing-store native fixtures were not rerun for this
step; their prior evidence remains historical. Strict Clippy passes in both locked
dependency graphs, the metadata-only publication build still compiles, and format
and diff checks pass. Hosted CI has not run. The three-page captured evidence and
frozen standalone tool live outside Git under the local issue evidence profile.

This does not yet connect part definitions to exact committed native records or
upstream retained task dependencies. Native scheduling, native issue/packet
restoration and deterministic dispatch remain open, along with the broader
[completion ledger](../workflow-completion.md). Source/changed-path and actor
bounds are unchanged; a nominally valid part can still fail native admission.
This is not BBJ/Melena adoption or an asset/profile migration.

Applied producer checklists for comics, story/editing, platform/audience and
rights/provenance are recorded in the role check. No independent reviewer,
creative/translation approval, font permission, print proof or release is implied.
The PDF is a private raster reading derivative; text/SVG/editable sources remain
separate, and issue-level and neighboring-page review remain project obligations.
