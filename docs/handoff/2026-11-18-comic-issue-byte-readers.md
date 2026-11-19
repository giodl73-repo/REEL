# Comic issue assembly from declared byte readers

`reel-publication::comic_issue::capture_with` accepts the assembly request and a
bounded reader, replays all declared part packets and returns the complete
editable issue packet in memory. It shares the assembly implementation with the
filesystem command. No workspace, provider, native publication or actor effect
is performed by this API.

The reader must provide each part's captured editable sources, technical receipt
and named outputs. Parts retain their master, edition, language, page order,
panel/utterance order and font/license correspondence checks. Generated reading
PDFs, accessible source transcripts, relocated controls and editable parts remain
separate files. The existing aggregate input/output limits still apply.

The three-page/two-part test removes original source/part folders, assembles via
the CLI, then supplies only an in-memory map to the new API. The entire resulting
file map must equal the CLI packet. Changed lettering and unavailable plan bytes
refuse. Existing directory inventory and destination-overlap checks remain on
the filesystem command.

Producer-applied comics/editorial checks retain spread and reading order;
platform/audience checks retain separate reading/editable derivatives; rights
checks retain source and font/license identity. No checklist grants creative,
translation, print or release approval. Native issue-tool binding, admission,
retained dependency reads and final issue retention remain subsequent work.

Private qualification evidence belongs in
`F:/src/tig-dev-temp/reel-workflow-issue-capture-20261118-v1`.
The focused complete-packet replay test, all 63 shared workflow/publication/print
tests, strict Clippy and `git diff --check` passed. These checks qualify the
byte-reader assembly seam; they do not qualify native issue production.

For the native adapter, part results expose named reading/page outputs and the
worker receipt; editable sources are not automatically additional result
artifacts. An assembly task must explicitly declare every required source as
well as part receipts/outputs, retaining its original Git or upstream logical
origin. A receipt's source-object list cannot manufacture input custody.

The adapter must apply its existing limits in addition to the local assembler's
limits: 128 declared inputs, 64 MiB input closure, 128 changed paths per native
phase and owned staging's 256-file bound. The portable 128-part issue grammar
does not promise that every issue fits a single native job. Refuse oversized
closures and require explicit grouping; do not widen limits or hide extra reads.
