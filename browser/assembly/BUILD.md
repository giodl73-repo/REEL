# REEL assembly source and build

See browser/assembly/README.md in source.zip for exact compiler/bindings, commands,
limits and interpretation. Rust 1.95.0, wasm-bindgen CLI 0.2.127, Python 3.11+,
Node 22 and Playwright 1.61.1 are required. Set an absolute CARGO_TARGET_DIR;
no TRACKER-relative path or media cache is required. Original native CLI controls
compile with the root Cargo.lock; the browser adapter lock retains all shared
production versions, sources and checksums. Cargo fetches dependencies: this is
not an offline vendored dependency archive.

Compiled WASM is pkg/reel_assembly_workbench_bg.wasm; its bindings are
pkg/reel_assembly_workbench.js. These are distinct from complete first-party Git
source, including original native sources/tests and committed vendor notices,
in source.zip. Exact file hashes are in source-files.json. Root scoped software,
content, vendor and prior grants remain intact. Software/ordinary software docs
are MIT; original non-software content retains CC BY-NC 4.0. All first-party and
resolved WASM dependency notices are retained separately from Rust 1.95.0 notices.

The static package stages explicit runtime, source, legal and provenance files
only. Native test requests, CLI executables, Cargo metadata, screenshots and logs
stay outside the served tree. Exact Git source and served inventory are checked
before and after artifact transfer; the site budget is 5 MB. Source replay uses
an extraction path with spaces outside every Git checkout and verifies first-party
manifest containment. Cross-directory binary identity is not claimed; native and
WASM results must agree exactly. Original production CI remains separate.

This browser is a semantic dependency diagnostic. No media is read or restored;
no renderer runs. Graph validity, pointer declarations, computed graph identity,
available media and human approval are distinct. Exported user reports contain
identities and are not privacy-safe release receipts. Default-branch hosted
qualification and fresh live verification remain separate from local checks.
