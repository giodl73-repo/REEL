# Pinned source dependencies

`tig/` contains unmodified Cargo manifests, lock, Rust source and embedded catalogue
web assets from Tig at the
exact commit in `tig/source-pin.json`, with its unchanged MIT license. It is a
nested workspace excluded from REEL workspace membership. Tig is the upstream
authority: fixes go upstream and the pin is updated explicitly; no local fork.

Vendoring permits reproducible builds without cross-repository credentials or
a separately installed Tig checkout. Upstream is the public
[giodl73-repo/tig](https://github.com/giodl73-repo/tig) repository.
The pin lists raw lengths/SHA-256 for all 115 imported files
(1258310 bytes). Embedded web assets are compile-time `include_str!` dependencies;
vendoring them does not start a catalogue service. External tests and design
documents are omitted. Consumer
tests reject changed or unlisted files. This is no cache initialization
or shared runtime installation.

Current source [0ceade2a16de1c1ed6cc27e48675e7945d7b61df](https://github.com/giodl73-repo/tig/commit/0ceade2a16de1c1ed6cc27e48675e7945d7b61df)
is the public baseline after Tig PR1, with passing upstream Windows/Linux CI.
Only source blobs are imported; no upstream Git history is included.
It includes bounded native
source/graph/recipe/job and existing-byte interfaces, plus complete job evidence
within explicit whole-envelope tree budgets. Runtime names retain their separate
64-name bound. Pre-execution composition and input evidence use explicit named
dependency maps and exact replay admission. Composition has its own resource
role; neither map is reclassified as primary input or runtime. Empty dependency
maps preserve old job identities. Resource membership does not establish consumed
recipe meaning, producer freshness, payload availability or custody.

REEL must qualify this exact import on Windows/Linux/FFmpeg before merge. A source
pin or upstream pass alone is not consumer qualification, full semantic/custody
coverage, eviction authority or production cutover approval.
