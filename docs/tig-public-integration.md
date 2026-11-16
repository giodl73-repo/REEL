# REEL with public Tig

For the overall workflow, see [use cases](use-cases.md) and
[architecture](architecture.md#reel-and-tig). [Installation](setup/install.md)
maps the optional features and native runtime requirements.

REEL owns media interpretation, rendering and independent media checks.
[Tig](https://github.com/giodl73-repo/tig) owns asset records, exact byte identities,
Git snapshots, guarded resolution and storage lifecycle primitives. Scene controls
refer to logical asset IDs; the REEL adapter resolves those IDs through Tig at one
explicit Git commit and verifies the expected SHA-256 and size before use.

```text
project Git commit       existing byte stores
  definitions + .tig          exact hashes
           |                       |
           +---- Tig resolution ---+
                       |
              REEL scene compiler
                       |
              guarded render + check
                       |
             retained output candidate
                       |
             reviewed Git proposal
```

## Build and feature boundary

REEL vendors unmodified Rust source from public Tig commit
`0ceade2a16de1c1ed6cc27e48675e7945d7b61df`, including its MIT license and
[per-file hashes](../vendor/tig/source-pin.json). No private repository access,
separate Tig checkout or background service is needed to build these adapters.
Use a current stable Rust toolchain. FFmpeg and FFprobe are external dependencies
for the rendering example below.

| Cargo feature | Interface | Guide |
| --- | --- | --- |
| `tig` | Exact input resolution and native scene compilation/render/check | [Backend](tig-backend.md), [scene compilation](tig-native-scene.md) |
| `tig-publication` | Managed jobs, retained checked outputs and candidate handoff | [Job runner](tig-job-runner.md) |
| `tig-history` | Revision/delivery history through REEL's explicit CAS publisher | [History host](tig-history.md) |

```sh
cargo build --locked --features tig-history --bin reel-tig-scene --bin reel-tig-job --bin reel-tig-history
```

## Try the two-version example

From the REEL repository on Windows, with Git, FFmpeg and FFprobe on PATH:

```powershell
# Keep build and temporary space on your chosen development volume.
$env:CARGO_TARGET_DIR = 'F:/build/reel-tig-example'
$env:TEMP = 'F:/tmp'
$env:TMP = 'F:/tmp'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_INCREMENTAL = '0'
cargo test --locked --features tig --test tig_inputs native_tig_cli_renders_two_cels_shared_cue_then_replaces_only_cel_b -- --ignored --exact --nocapture
```

Choose existing directories appropriate to your machine; `F:` is only an example.
The fixture creates a temporary Git repository, small generated pictures and a
synthetic recording. It plans and renders two cels sharing one cue, commits a
replacement for cel B, rejects the stale job, updates its expected pin and
renders the replacement. Independent decoded comparisons prove that only cel B's
frames change, with the shared audio unchanged. It also tests retained source
replay with the fixture Git source unavailable. All media and stores are
temporary fixture data.

The broader synthetic lifecycle test exercises changed-scene reuse, parallel
publication, delivery retention and verified eviction in fixture caches:

```powershell
cargo test --locked --all-features --test tig_inputs fake_episode_lifecycle::fake_episode_runs_v1_then_rebuilds_only_the_changed_scene -- --ignored --exact --nocapture
```

Both fixtures run in REEL's Windows CI. Ordinary all-feature tests and Clippy run
on Windows and Linux. Path-based mandatory protection currently requires Windows;
unsupported hosts refuse rather than silently weaken protection.

## Connect a project

Keep project profiles and cache roots in the producing project, outside this
generic REEL repository. Follow the [explicit profile](tig-backend.md#explicit-use)
and [native scene request](tig-native-scene.md) contracts. Capture a commit,
admit exact definition objects, select payload streams explicitly and pass logical
IDs with exact expected byte pins. A changed selected input invalidates an old job;
recompile before rendering the new revision.

Rendering creates a candidate. Creative selection, approval, delivery and Git
application remain separate explicit steps. Optional publication/history hosts
require their own enrolled roots and policies; the input reader alone grants no
eviction authority. This integration does not install shared tools, change live
project bindings, create caches or enable automatic transfer/removal. Full custody
coverage and production adoption remain project-specific qualification work.
