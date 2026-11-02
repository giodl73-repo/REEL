# REEL semantic assembly workbench

Inspect an owned native `reel.semantic-assembly.v1` graph, node/presentation target
and optional `reel.selected-pointer.v1` pointer using the existing Rust
`reel-assembly` crate. Start with the wholly authored synthetic graph; its asset
hashes are declarations with no bundled media and its zero declared graph lock
intentionally differs from the separately computed lock. The example is not a
project production record or a release-ready package.

Native `validate_graph`, `closure`, `selected_closure`, `presentation_closure` and
`graph_lock` own all contract/selection/hash behavior. No JavaScript graph
algorithm, duplicated DTO, cache reader, TIG runtime or renderer is introduced.
The core, root manifests/lock, native CLI and native workflows remain unchanged.
The standalone lock retains every shared production version/source/checksum,
including serialization and digest dependencies.

## Interpret the facts separately

Native structural validation checks declared IDs, references, revision histories
and event constraints. It does not prove whole-graph acyclicity, selected media
availability or production readiness. Closure checks cycles reachable from its
selected node, omits unselected slots, and returns native sorted node/asset/event
results without fetching any cache URI. All graph slot dispositions remain visible,
including explicit silence and unselected slots outside the requested closure.

Pointer matching compares the supplied graph lock identity. The native computed
graph lock is shown as a separate observation. Matching declarations or computed
metadata does not verify media bytes, source provenance, rights, consent, creative
selection or release authority. Reports contain caller-supplied identities and
are local working diagnostics, never privacy-safe release receipts. No input is
uploaded, stored across sessions or used to initiate production work.

## Local authoring and bounds

Open a graph as UTF-8, edit current text, choose node/presentation and target ID,
and optionally paste the original pointer JSON (exactly empty means omitted).
Original graph bytes retain BOM/CRLF independently from current text and explicit
LF copies. Target/mode/pointer edits invalidate previous reports and older worker
or file/example completions. Request export preserves graph text and all controls;
report export retains native structures and observations. Original/current/request
exports remain usable without the Rust engine.

Workbench admission: 128 KiB UTF-8 graph, 8 KiB pointer, 256 target bytes, 128 nodes,
256 slots/events/presentation targets each, 1024 total node references and 1024
retained revisions. The byte budget can bind before item maxima. Unknown native
fields reject whole input. Native execution uses a 64 MiB compiled WASM memory
maximum; reports have a 2 MiB budget. Read/startup/computation deadlines are
5/12/10 seconds. Nothing is truncated, sampled, silently relocked or restored.

## Qualification and source

Rust 1.95.0, wasm32-unknown-unknown, wasm-bindgen CLI 0.2.127, Python 3.11+,
Node 22 and Playwright 1.61.1. Set CARGO_TARGET_DIR to an absolute build directory;
use CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0 and CARGO_INCREMENTAL=0
for the same bounded build profile. No TRACKER path or external media is needed.

```text
cargo +1.95.0 test -p reel-assembly --all-targets --locked
cargo +1.95.0 test --manifest-path browser/assembly/Cargo.toml --locked
cargo +1.95.0 clippy --manifest-path browser/assembly/Cargo.toml --locked --all-targets -- -D warnings
python browser/assembly/build-prototype.py --bindgen /absolute/path/to/wasm-bindgen
python browser/assembly/package-site.py
python browser/assembly/verify-site.py target/reel-release/REEL/assembly --commit <HEAD>
```

From browser/assembly: `npm ci --ignore-scripts`, `npx playwright install chromium`,
`npm run test:pages`. Set REEL_SITE_DIR=../../target/reel-release for the complete
package; local prototype behavior uses ../../target/reel-prototype. Port 4195
serves `/REEL/assembly/`. Delivery requires every full package check.
`python browser/assembly/verify-source.py --bindgen /absolute/path/to/wasm-bindgen`
extracts the exact source outside Git into a path with spaces, checks manifest
containment and production dependency identities, repeats the 51 assembly and
four adapter tests, rebuilds the native/WASM adapter plus original production
CLI, and repeats exact native/CLI/browser behavior. Only the two original package
delivery checks are excluded from rebuilt behavior. Full native production CI,
FFmpeg, media rendering and owner acceptance are separate gates and claims.

The native workbench CLI reads one JSON request from stdin, with string fields
`graph_text`, `target`, `kind` and `pointer_text`; native transport is at most
2 MiB. Invalid transport/UTF-8 exits nonzero. The inner Rust entry returns a JSON
diagnostic for admitted requests. The original native CLI is used independently
for selected closure controls; browser operation needs neither CLI nor FFmpeg.

Status, role review, native compatibility boundaries and exact release evidence
belong to `context/waves/2026-11-02-semantic-wasm/`.
