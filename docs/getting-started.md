# Getting started

This path introduces the main CLI using checked-in synthetic controls. It needs
Rust and Git, with no media cache, provider account or renderer for the first two
steps. Run commands from the REEL repository root. Use a current stable Rust
toolchain; [installation](setup/install.md) covers optional executables and platforms.

## Validate a small example

```sh
cargo build --locked --bin reel
cargo run --locked --bin reel -- validate manifests/fixtures/two-speaker-untimed/planning.yaml --output json
cargo run --locked --bin reel -- plan manifests/fixtures/two-speaker-untimed/planning.yaml --output json
```

Validation checks the manifest contract. Planning reports the proposed structure
and export choices. Read the fixture's `planning.yaml` to see the work identity,
scenes, narration cues, speakers, captions and timing state. A valid plan may
still be intentionally untimed and unsuitable for rendering.

## Conform measured timing

The fixture includes measured cue timing. This command writes a new conform
packet; choose a new output directory each time:

```sh
cargo run --locked --bin reel -- conform manifests/fixtures/two-speaker-untimed/planning.yaml --cues manifests/fixtures/two-speaker-untimed/cue-measurements.yaml --speaker-tempo narrator=85 --output-dir target/quickstart-conform
```

Inspect the conformed manifest, captions and lineage in that packet. Conform
derives timing from the declared measurements and policies. It does not record
voices, generate pictures or approve the result. See
[production manifests](production-manifest-v0.2.md) and
[cue import](cue-import-and-continuity-v0.1.md).

## Choose the production route

| Your inputs | Route |
| --- | --- |
| A timed production manifest and explicit local media | [Legacy animatic verification](animatic-verification-v0.2.3.md); check its [render environment](render-environment-v0.2.4.md) before rendering. |
| Native scene, measured cues, bindings and exact pictures/recordings | [Scene authoring](scene-authoring-v1.md) and [native scene delivery](scene-delivery-v0.1.md). |
| Committed Tig records and existing byte stores | [Public Tig integration](tig-public-integration.md), then [native compilation](tig-native-scene.md) and [managed jobs](tig-job-runner.md). |
| Retained scene/presentation masters for an episode | [Episode planning](tig-episode-planning.md) and [bounded episode execution](tig-episode-jobs.md). |

These are distinct contracts. A legacy `reel.manifest.v0.2` YAML file is not a
native scene JSON document or a Tig `.tig` instance. Select the matching adapter
rather than renaming a file or inventing a wrapper script.

## Try a rendered Tig lifecycle

The [public Tig guide](tig-public-integration.md#try-the-two-version-example)
contains an isolated Windows fixture: two generated cels share one synthetic
recording; a committed replacement changes only cel B; stale input pins refuse;
the old source view can be replayed with Git offline. It creates temporary
fixture stores instead of using a real project's caches.

Path-based mandatory protection currently requires Windows. Library/planning
and portable contract tests run on Linux too; use each guide's explicit platform
requirements before attempting a production job.

## Bring your own project

Keep canonical text, source authority, definitions, scene order and creative
decisions in the producing project. Supply explicit file pins or Tig IDs,
language/profile choices, runtime pins, scratch budgets and new output locations.
Keep machine-specific cache roots and raw reports private. Recheck media before
selection or delivery; validation of a plan alone is insufficient.

For worked workflow choices, see [use cases](use-cases.md). For the division of
responsibility between project, REEL, Tig and external tools, see
[architecture](architecture.md).
