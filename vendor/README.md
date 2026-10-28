# Pinned source dependencies

`tig/` contains unmodified Cargo manifests, lock and Rust source from Tig at the
exact commit in `tig/source-pin.json`, with its unchanged MIT license. It is a
nested workspace excluded from REEL workspace membership. Tig is the upstream
authority: fixes go upstream and the pin is updated explicitly; no local fork.

Vendoring permits reproducible private-repository CI without cross-repository
credentials. The pin lists raw lengths/SHA-256 for all 65 imported files
(691949 bytes). External tests and design documents are omitted. Consumer
tests reject changed or unlisted source/manifests. This is no cache initialization
or shared runtime installation.

Current source 4e80e0116e36567d1e1f10dbc6c6acf742e2f192 is the qualified merge of Tig PR47. Its entire tree
matches the exact Windows/Linux-tested f3cf9b5 head. It includes bounded native
source/graph/recipe/job and existing-byte interfaces, plus complete job evidence
within explicit whole-envelope tree budgets. Runtime names retain their separate
64-name bound; evidence is not reclassified as canonical primary input.

REEL must qualify this exact import on Windows/Linux/FFmpeg before merge. A source
pin or upstream pass alone is not consumer qualification, full semantic/custody
coverage, eviction authority or production cutover approval.
