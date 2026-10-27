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
produced-claim replay (upstream PRs 22 through 27), plus canonical Windows cache
roots (PR28). The 50 imported files total 537,367 raw bytes. The exact upstream
96177878c7db238958c20a5faf1eec9bcfd6f438 pin must pass its Windows/Linux
qualification. Upstream PR28 and REEL consumer qualification are required before
merging this import; the source pin alone is not qualification evidence.
Native record and definition envelopes remain unchanged. These APIs do not
recursively infer opaque native semantics, prove producer consumption, establish
global source/job coverage or grant cache-removal authority.
