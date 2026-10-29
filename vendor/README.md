# Pinned source dependencies

`tig/` contains unmodified Cargo manifests, lock and Rust source from Tig at the
exact commit in `tig/source-pin.json`, with its unchanged MIT license. It is a
nested workspace excluded from REEL workspace membership. Tig is the upstream
authority: fixes go upstream and the pin is updated explicitly; no local fork.

Vendoring permits reproducible private-repository CI without cross-repository
credentials. The pin lists raw lengths/SHA-256 for all 65 imported files
(694972 bytes). External tests and design documents are omitted. Consumer
tests reject changed or unlisted source/manifests. This is no cache initialization
or shared runtime installation.

Current source 9252a8a1822e99155e5d2bc9bdd4a96476cfe22f is the qualified merge of Tig PR48. Its entire tree
matches the exact Windows/Linux-tested 9660617 head. It includes bounded native
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
