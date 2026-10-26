# Native Tig backend

Implementation scope: an optional Rust `tig` feature and explicit-profile
`reel-tig-scene` binary consume committed Tig IDs through the existing scene
input-provider interface. The source dependency is pinned and vendored; see
`vendor/README.md`. No private CI credentials, absolute Cargo paths, copied media,
new cache, daemon, transfer or removal operation is introduced.

The application captures one explicit Git commit and namespace scope. Each
admitted definition is pinned by exact ID, SHA-256 and size and declares a payload
selector: selected object or one named stream. There is no implicit language,
latest-version or historical-stream fallback. Asset input lists come from the job
and from its exact selected derivation receipts; they are not maintained again in
the profile. Definition admission does not execute arbitrary schema semantics or
grant creative approval.

The backend preflights expected pins, verifies bytes through Tig, and owns guards
through synchronous rendering and independent checks. Byte readers can supply
bounded alignment JSON in cooperative mode. A path-based render requires mandatory
path protection, currently Windows only. Unsupported protection refuses before
output creation; it does not downgrade. Complete source/job claim recovery,
custody/eviction authority, native family parity and production adoption remain
separate obligations.

## Explicit use

Build with `cargo build --features tig --bin reel-tig-scene --locked` (Rust 1.89+
for this optional feature). The ordinary build remains usable without Tig.
The release workflow includes the new binary when a release is explicitly tagged;
adding this source does not install it or change a project's runtime binding.

```powershell
reel-tig-scene --profile .local/reel-tig.json plan .local/scene-job.json
reel-tig-scene --profile .local/reel-tig.json render .local/scene-job.json renders/new-scene
reel-tig-scene --profile .local/reel-tig.json check .local/scene-job.json renders/new-scene
reel-tig-scene --profile .local/reel-tig.json render-audio .local/scene-job.json renders/new-audio
reel-tig-scene --profile .local/reel-tig.json check-audio .local/scene-job.json renders/new-audio
```

Outputs must be new. Job and profile files are private local inputs; keep profiles
in the producing project's ignored `.local` directory. The result
JSON reports the captured metadata checkpoint, input-name/verified-object counts,
the normal REEL plan/receipt, and `complete_semantic_coverage: false`. Cache paths
are not added to that result. Operational errors remain private diagnostics.

Each job `FileRef.path` is the exact portable Tig ID, such as `cel/ending` or
`cue/014`; its hash and byte count remain mandatory. Job/contract/production
metadata are still explicit local files, not automatically materialized from Tig.
The CLI resolves a relative job filename to an absolute file before REEL uses
its parent directory, preserving local contract-relative lookup.

The profile schema is `reel.tig-input-profile.experimental-1`. Replace every
placeholder below with the project's actual pinned definition/commit and existing
local paths. `bytes` is the exact definition-file length, not a media budget.

```json
{
  "schema": "reel.tig-input-profile.experimental-1",
  "git_executable": "C:/Tools/Git/cmd/git.exe",
  "repository": "C:/work/project",
  "commit": "<full-commit-id>",
  "records": ["episodes", "shared"],
  "definitions": ["definitions"],
  "admitted": [
    {
      "id": "types/cel",
      "object": {"sha256": "<definition-sha256>", "bytes": 123},
      "payload": {"kind": "selected"}
    },
    {
      "id": "types/cue",
      "object": {"sha256": "<definition-sha256>", "bytes": 456},
      "payload": {"kind": "stream", "slot": "es"}
    }
  ],
  "byte_source": {
    "places": [
      {"root": "F:/project-cache", "layout": "reel-cas"},
      {"root": "D:/project-history", "layout": "reel-cas"}
    ],
    "required": "prevent-mutation"
  }
}
```

An episode profile can choose an existing F cache first; a video profile can
choose E first. These are explicit per-operation roots and do not fill or create
those caches. A corrupt present object refuses instead of falling through to a
later root. A named archive/index does not by itself make an object a readable,
verified replica. Supported existing layouts come from Tig's byte provider.

The library `Source::read_alignment` adapts directly to
`read_verified_alignments_from_assets`; a cooperative profile supports bounded
read/seek metadata consumption on Unix as well. `Source::prepare` returns an
`Inputs` owner implementing `scene_delivery::InputFiles`. It deduplicates guards
by object digest across input names and retains them after the source/catalog is
dropped. Keep that owner alive until the complete operation returns. Definitions
and stream selectors are admitted policy, not a repeated list of assets.

## Bounds and verification

Profiles are at most 64 KiB, with duplicate/unknown fields refused. There are at
most 256 admitted definitions and 64 paths per namespace scope; Tig separately
bounds Git metadata capture. Jobs are at most 4 MiB; preparation allows at most
4,096 logical names and is further limited by Tig's byte/request/live-reader
budgets. Derivation receipts are at most 4 MiB each/16 MiB aggregate. All direct
expected pins are preflighted before byte access; derivation component pins are
resolved from the exact verified receipt before rendering. A failed preparation
drops partial guards while consumed verification budgets remain charged.

Tests verify the vendored raw bytes, actual committed Git capture, exact admitted
definitions and explicit language streams, native alignment consumption,
corruption refusal, profile bounds, mandatory-mode refusal, partial guard release,
deduplication, derivation components and historical views. The Windows-only real
CLI fixture exercises plan/render/render-audio/check/check-audio, one shared cue
and a cel-B-only change with exact pixel/PCM and historical-replay checks. Other
platforms test read/seek behavior and mandatory-path refusal explicitly.

Normal subprocess completion is covered. Abnormal child/process recovery and
durable source/job claims are not established by this read-only adapter. Rendering
does not authorize selecting, publishing, demoting or deleting anything.

Local Windows qualification (2026-10-26): the all-feature workspace run passed
976 tests with 37 existing external-tool cases ignored. After strengthening the
deduplication fixture and adding the receipt-size preflight case, the final Tig
target passed 12 tests with its one external-tool case ignored; that real CLI case
was then executed explicitly and passed. Workspace all-target/all-feature Clippy
with warnings denied, the no-default-feature library build, formatting and
whitespace checks pass. Current-head Windows/Linux CI remains required for merge.
