# Install the tools for your workflow

REEL is a Rust workspace with a main CLI and several separate executables.
Choose the interface described by your [use case](../use-cases.md). Build from
one explicit source revision and record the actual tool identity in production.
Source on `main`, a tagged release and an installed project tool can differ.

## Main CLI

From a checkout, using a current stable Rust toolchain:

```sh
cargo build --locked --bin reel
cargo run --locked --bin reel -- --version
```

The executable is under `target/debug/` unless `CARGO_TARGET_DIR` is set. Use
`--release` for a release build. To install just the main CLI into Cargo's bin
directory:

```sh
cargo install --path . --locked --bin reel
reel --version
```

[Getting started](../getting-started.md) validates and plans a checked-in fixture
without media tools, providers or cache setup.

## Optional features and separate tools

| Workflow | Source build |
| --- | --- |
| Tig input resolution and scene compilation/render/check | `cargo build --locked --features tig --bin reel-tig-scene` |
| Managed Tig media jobs | `cargo build --locked --features tig-publication --bin reel-tig-job` |
| Tig history host (includes publication/input features) | `cargo build --locked --features tig-history --bin reel-tig-history` |
| Native print collections | `cargo build --locked --features print --bin reel-print` |
| Native albums | `cargo build --locked --bin reel-album --bin reel-album-video` |
| Cel editor and explicit local host | `cargo build --locked -p reel-assets --bin reel-cel-sorter --bin reel-editor-host` |
| Note and lyric-underlay editor | `cargo build --locked -p reel-music --bin reel-music-editor` |

The `song-provider` feature enables the contained original-song provider host;
its [provider contract](../original-song-repair-provider.md) requires separately
declared runtimes and authority. It does not install a model or provider.

The [Tig source](../../vendor/README.md) is vendored at an exact public commit.
Building these adapters needs no private Tig repository access or separately
installed Tig checkout. Profiles, admitted roots and runtime dependencies still
come from the producing project.

## Media and platform dependencies

| Route | Requirements |
| --- | --- |
| Legacy `reel` animatic/preview transport | FFmpeg/FFprobe; the legacy Windows adapter uses WSL, while Linux runs native programs. Use `reel render-doctor --output json` for that adapter. |
| Native scene, album and episode operations | Explicit native FFmpeg/FFprobe paths and required capabilities, as specified by the operation/profile. The native runtime binds program identities rather than using the legacy WSL adapter. |
| Managed native runtimes and strict Tig path guards | Windows mandatory path protection; unsupported protection fails before rendering instead of weakening the guard. |
| Contained native browser worker | Windows, an explicitly admitted Chromium-compatible browser and pinned native media programs. |
| Native print | The Rust `print` feature, supported pinned sources and explicit font files; no browser or Python runtime is required for this renderer. |
| Legacy recognition/engraving/collection adapters | Their documented Python, model, font or external application dependencies; see the relevant contract. |

CI uses a pinned FFmpeg build. A different local build must pass the chosen
operation's capability and media checks; finding an executable is insufficient.
FFmpeg/FFprobe and browsers remain external dependencies, not bundled guarantees.

The [local editor host](../editor-host-v1.md) is explicitly launched for an
exported editor session. The [hosted WASM workbench](https://giodl73-repo.github.io/REEL/assembly/)
needs a browser and inspects semantic graphs without a native install. Neither
surface is the managed browser frame renderer.

## Tagged binaries

Inspect [GitHub Releases](https://github.com/giodl73-repo/REEL/releases) for the
actual assets attached to a chosen tag. The current release workflow uploads
the main `reel` and `reel-tig-scene` binaries for Windows/Linux x86-64; older tags
may differ. Do not assume a release contains every optional executable or every
capability currently on `main`.

Downloaded Rust executables need no Rust compiler, but rendering still requires
their external runtime dependencies. On Windows, place the chosen executable in
a directory on `PATH`; on Linux, mark it executable and use a directory on `PATH`.
Confirm its version/help and exact binary pin before adopting it for a project.

## Build and scratch locations

Set `CARGO_TARGET_DIR` to a development volume with sufficient space. Native jobs
also require explicit scratch/output budgets and private machine profiles.
Building source does not enroll stores, launch a daemon or select project media.
Keep old admitted executables available for frozen jobs that require them; new
source compilation is a separate runtime-adoption decision.
