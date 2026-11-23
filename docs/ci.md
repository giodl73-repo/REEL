# CI scope and timing

The normal PR target is **30 minutes or less after runners start**, with each
parallel Rust qualification job bounded to 28 minutes. This is a performance
target to measure, not a claim that cold builds or runner queues always fit it.
Timeouts fail checks rather than silently omitting tests.

## What is required

The existing required checks remain `rust (ubuntu-latest)`,
`rust (windows-latest)` and `ffmpeg-linux`. Their lightweight gates require
successful path classification and the applicable qualification jobs. A failure,
cancellation or unexpected skip cannot become a successful full-code gate.
The Rust gates aggregate the complete platform/shard matrix.

README and `docs/` prose/image-only changes use the documentation fast path.
Source, build, CI, configuration and executable-example changes run:

- Rust formatting, linting, library/binary/example/benchmark and integration tests;
- standalone assembly, music, print and asset-crate checks and editor UI tests;
- real synthetic frame/sample, caption, sound, motion and composition checks;
- Windows native protection, archived-source, retained-version and episode
  lifecycle qualification, including the pinned original-producer comparison;
- the FFmpeg capability/receipt suite and browser/WASM source-rebuild workflow.

This change reorganizes existing coverage. It does not move slow checks to an
optional nightly run, weaken native protection or change production code.

## Why the previous layout was slow

The completed [Windows run for PR367](https://github.com/giodl73-repo/REEL/actions/runs/44786488240)
spent about 30 minutes on ordinary tests, then ran all native qualification
serially. Source guards/archived-source work alone took about 16 minutes, followed
by retained-version comparisons and multiple complete episode fixtures.
The subsequent PR368 Linux job completed in about 24 minutes and FFmpeg in about
5 minutes; Windows remained the critical path.

## Parallel layout

| Lane | Platforms | Work |
| --- | --- | --- |
| Core 0–2 | Linux and Windows | Three balanced ordinary-test shards per platform; shard zero also runs non-integration targets, formatting and linting |
| Media | Linux and Windows | Existing common media, standalone-crate, editor UI, caption and motion checks |
| Native source | Windows | Existing guarded input, archived-source and native handoff tests |
| Native retained | Windows | Pinned original-producer build and retained-version comparison |
| Native episodes A | Windows | Component ownership, edit/rebuild, retained scene and bounded episode tests |
| Native episodes B | Windows | Mixed programmes, repeated-source episode and browser opening/credits tests |

These are twelve independent Rust jobs, alongside the existing FFmpeg and browser
jobs. Every qualified native command stays present with its existing runtime
requirements and test arguments. The original-producer build is needed only in
the retained lane; the Tig proposal CLI is prepared in the native lanes.

`.github/scripts/run-test-shard.cjs` discovers root integration targets from
`cargo metadata`, then assigns each exactly once using measured public-run
weights. Unknown/new targets receive a default weight instead of being omitted.
Shard zero preserves the remaining `--all-targets` scope. A change to default
workspace membership fails explicitly until coverage is updated. Inspect the
current assignment without building or rendering:

```sh
node .github/scripts/run-test-shard.cjs --plan
```

The weighted ordinary-test estimates are roughly 13–14 minutes per shard before
compilation; the longest native group previously took about 16 minutes before
its separate setup/build overhead. Hosted results must confirm the total target.
Parallelism trades additional concurrent runner capacity for shorter wall time;
GitHub account limits and queues can still delay completion.

## Validate changes to CI

```sh
node --test .github/scripts/*.test.cjs
git diff --check
```

Also lint the affected workflows with actionlint. Tests cover complete Git-diff
classification, required-gate failure behavior, complete deterministic test
assignment, and reachable/nonduplicated qualification steps. Keep concurrency
scoped by PR branch so a new optimization PR does not cancel another PR's run.
