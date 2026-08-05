# Production hardening requirement matrix

| Requirement | Implementation | Verification |
|---|---|---|
| Untimed scenes/shots | `TimingStatus`, optional timing, production validator/planner | Unit and CLI fixture tests |
| Precise render gate | `require_preview_ready` and `timing not conformed` errors | Untimed unit test and CLI dry-run boundary |
| Atomic voice conform | `production::conform` with same-parent staging and final rename | Selective-tempo unit test and CLI integration test |
| Per-speaker tempo | Repeated `--speaker-tempo SPEAKER=PERCENT` | Narrator 85%, poet 100% fixture |
| Protected pause | Locked millisecond `protected_pauses` | 1500 ms invariant in 7500 ms fixture |
| Caption synchronization | Cue timeline to generated SRT | Conform packet integration test |
| Transform lineage | Manifest/cue/audio/output/caption hashes and tool version | `lineage.json` assertions and packet inspection |
| Source/omission provenance | Source ranges, refs, omissions, bridges, coverage report | Coverage unit test |
| Privacy-safe continuity | Local asset policies and path-free provider package | Provider serialization unit test |
| Provider execution gate | Requested assets require approved policy and reference | Provider package blocker logic |
| Variant selection | Parent/root/scene/reason/dimensions/candidate/approval plus separate findings | Deterministic timestamp/path selection |
| Long-still controls | Hold, focal, crop, depth, screen direction, eye line, no-lip-sync, A/B checks | Quality report and filter unit tests |
| Clean A/B outputs | Manifest-declared named narration/effects audio variants | CLI integration dry-run creates three artifact plans |
| Asset-backed renderer | FFmpeg still inputs, bounded motion, xfade, captions, disclosure | Windows/WSL 7.500 s render and Linux CI render |
| Artifact provenance | Input/output hashes, bytes, duration, tool version, exact arguments | Renderer report plus ffprobe check |
| Legacy migration | New-file-only v0.2 migration, unknown-field retention, normalization, cue lifting | 12/12 live BERTICA migration audit |
| CI and release | Windows/Linux fmt, Clippy, tests, Linux FFmpeg E2E, tagged binaries | Workflow definitions |
| Format documentation | Six original grammars plus episodic-series grammar | Tracked format documents |
