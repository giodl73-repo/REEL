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
