# Pinned source dependencies

`tig/` contains unmodified Cargo manifests and Rust source from Tig at the exact
commit in `tig/source-pin.json`, with its MIT license. It is a nested workspace,
excluded from REEL's workspace membership. Tig remains the upstream authority;
fixes go upstream and the pin is updated explicitly. No local source fork edits.

Vendoring permits reproducible private-repository CI and optional binary builds
without granting cross-repository credentials to an Actions token. The pin lists
raw lengths/SHA-256 for every imported file. The Tig backend tests check those
bytes and reject unlisted Rust/manifests. Upstream external test suites and design
documentation are not imported; REEL tests the consumer boundary, and the upstream
commit has its own Windows/Linux qualification. Do not treat vendored source as
an independent cache implementation or as production cutover approval.

The current pin includes explicit map rules, metadata archives, cooperative reader
release, declared production requirements, frozen contexts, consumed recipes and
produced-claim replay (upstream PRs 22 through 27). The 50 imported files total
537,089 raw bytes. The exact upstream e3d80bb pin passed all four Windows/Linux
qualification checks before upstream PR27 merged. REEL consumer qualification
is additionally required before merging this import.
Native record and definition envelopes remain unchanged. These APIs do not
recursively infer opaque native semantics, prove producer consumption, establish
global source/job coverage or grant cache-removal authority.
