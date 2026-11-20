# REEL workflow — start here

REEL workflow is implemented across four Rust crates:

| Crate | Responsibility |
| --- | --- |
| `reel-workflow` | Plans, prompt packets, dependency identities, review, repair and composition contracts; CLI for private planning. |
| `reel-workflow-native` | Tig input protection, owned staging, deterministic workers, provider dispatch, retained results, status and original-operation recovery. |
| `reel-workflow-http` | HTTP/proxy provider adapters, including text, image and speech requests. |
| `reel-publication` | Publication contracts and comic page/issue production shared with REEL renderers. |

This is a development preview. The existing components and bounded synthetic
workflows are implemented; a producer-ready BBJ issue and S5 episode workflow
have not been delivered. The [completion ledger](../../docs/workflow-completion.md)
records the detailed boundaries. Start here instead of following its historical
qualification reports as operating instructions.

The current Windows bundle is
`F:/src/REEL-releases/workflow-preview-20261120-v4/START-HERE.md`. Its main
`reel-workflow.exe` is an optimized build with `comic` enabled. It has passed
actual BBJ preparation, exact page SVG composition and existing-output refusal;
the underlying publication adapter has completed the selected 28-sheet local
book. The complete selected reading PDF has also passed an exact pixel check
against all 28 selected PNGs. The bundle manifest pins both workflow binaries.

## Run the planning CLI

From the REEL checkout:

```powershell
cargo build --manifest-path crates/reel-workflow/Cargo.toml --locked --features comic --bin reel-workflow
```

With `reel-workflow.exe` on PATH, check the supplied synthetic two-task plan and
export the prompts for manual execution. Choose a new output directory:

```powershell
reel-workflow check-plan crates/reel-workflow/examples/manual-prompts.json
New-Item -ItemType Directory -Path ./workflow-example-run
reel-workflow export-task crates/reel-workflow/examples/manual-prompts.json comic-editorial-es ./workflow-example-run/comic-prompt.json
reel-workflow export-task crates/reel-workflow/examples/manual-prompts.json film-cel-plan-en ./workflow-example-run/film-prompt.json
```

Each packet contains the prompt, language, output contract and stable task key.
The example uses a synthetic commit and no media. It exercises prompt export,
not comic rendering, film assembly, provider execution or native job admission.
Returned bytes need a matching result descriptor and, for durable completion,
native ingestion and retained-owner acknowledgement. A prompt response alone
does not complete a production task.

## Compose a BBJ book

For an existing BBJ book, the [comic publication guide](../reel-publication/README.md)
provides `bbj-prepare` and `layered-compose-book`: actual selected SVG sources are
composed and compared, exact reading PNGs are reused, and a private reader plus
layer/source bundles are written with stable review IDs. The workflow also
exposes a `layered-comic-compose` native capability. One actual BBJ page has
completed native composition, retention, review export and close; the complete
issue/fix task graph remains to be qualified end to end.

`bbj-prepare` also writes a typed, validated `publication.json`, with stable
page/panel/dialogue IDs and explicit empty arrays for silent/matter pages.
Its `bbj/input/...` source IDs are prospective unselected candidates; preparing
the contract does not register them in Tig or adopt BBJ's selected profile.
Use `check-layered-task PACKET INPUT_BINDINGS` before native admission and
`replay-layered-task PACKET INPUT_BINDINGS` to verify the declared page bytes
locally. Neither command admits a task or acknowledges retained ownership.

The same operations are available from the main workflow CLI built with `comic`:

```powershell
reel-workflow bbj-prepare $selection F:/src/bbj my-issue en F:/work/my-issue-prepared
reel-workflow bbj-compose F:/work/my-issue-prepared F:/work/my-issue-review
```

Open the resulting `index.html`. Use a new directory; partial outputs are retained
on failure. `bbj-compose-page REQUEST INPUT_BINDINGS NEW_DIRECTORY` composes one
candidate SVG separately. These commands are local publication operations;
the native workflow lifecycle below governs protected production jobs.

## Prepare a managed BBJ page

```powershell
reel-workflow bbj-plan-page PREPARED 3 CONFIG.json NEW_CAPSULE_DIRECTORY
```

`CONFIG.json` has schema `reel.workflow-bbj-page-plan.v1` and declares `project`,
`source_commit`, an authored `task_prefix`, `attempt_id`, the qualified native
`executable` pin (`path`, `sha256`, `bytes`) and the intake's `native_inputs`
artifact. Obtain that last descriptor from the exact declaration exported by
`reel-workflow-native export-native-bindings` for the owned empty-input source
profile. Native admission will recheck that profile; the planner grants no
custody or runtime qualification.

The command checks the selected page's publication, source/provenance bindings,
file pins, runtime and bounded reading PNG before writing a new capsule:

| File | Use |
| --- | --- |
| `intake-plan.json`, `intake-packet.json` | Exact external-source intake task. |
| `upstream-empty.json`, `parents-empty.json` | Explicit empty root admission controls. |
| `incoming/`, `returns.json`, `proposed-return.json` | Captured private sources and proposed manual return; no retained completion. |
| `recipe.json` | Late-bound composition step for the native recipe runner. |
| `local-preview-packet.json`, `local-inputs.json` | Read-only page replay before admission. |

Run `replay-layered-task NEW_CAPSULE_DIRECTORY/local-preview-packet.json
NEW_CAPSULE_DIRECTORY/local-inputs.json` to verify the composed SVG and reused
reading pixels. After explicit intake admission/import/retention, the existing
recipe runner resolves exact retained parent outputs and computes dependency
digests in Rust. Supply the project's owned native runner profiles and enrolled
actors. No dependency hashes or source maps need to be reconstructed in a shell.

Capsule creation refuses an existing destination and preserves partial writes.
It does not recover an earlier admitted task under a fresh identity. A page
capsule is not a full issue, a new raster render, profile adoption or approval.

## Assemble a complete selected reading edition

Use the prepared book to capture every selected reading PNG, source definition
and authoring record in exact sheet order:

```powershell
reel-workflow bbj-plan-reading-issue PREPARED CONFIG.json NEW_CAPSULE_DIRECTORY
reel-workflow compose-reading-issue NEW_CAPSULE_DIRECTORY/local-preview-packet.json NEW_CAPSULE_DIRECTORY/local-inputs.json NEW_READING_DIRECTORY
```

The config has the same fields as the page planner, with schema
`reel.workflow-bbj-reading-plan.v1`, its own task prefix and a native runtime
supporting `comic-reading-assembly`. It writes the intake, manual-return controls
and late-bound native recipe in Rust. The local composition produces
`reading.pdf`, `reading-manifest.json`, `source-index.json` and `receipt.json`.
The PDF embeds the original opaque image pixels losslessly and fits each page
proportionally within 720 points; that display size is not a printer trim.
Page, panel and dialogue review targets retain their stable IDs. Review images
refer to the captured input artifacts and must remain protected through their
original lineage; they are not duplicate output candidates.

The intake is bounded to 120 files and 128 MiB. The reading task has an explicit
256 MiB input/output envelope, individual PNGs at most 16 MiB and a bounded
canvas. Native admission still needs owned profiles with sufficient declared
return/workspace budgets, actor/path capacity and retained coverage. A local PDF
does not establish native retention. The source index explicitly reports that
editable layer payloads have not been restored by this reading-only operation;
the layered source archive and printer/accessible derivatives remain separate.

## Run protected work

Build the separate native workspace explicitly:

```powershell
cargo build --manifest-path crates/reel-workflow-native/Cargo.toml --locked --features film --bin reel-workflow-native
reel-workflow-native --help
```

The [native bridge guide](../reel-workflow-native/README.md) documents the
existing commands. A project supplies exact input records, its owned machine
profile, enrolled actor configuration and provider or manual-handoff settings.
Those are not inferred from a checkout or borrowed from another production lane.

The implemented path is plan/packet -> input preview -> admission -> owned
staging -> manual/provider/tool work -> candidate import -> retained completion.
`dispatch-task` operates on one bound task; `advance-recipe` advances a declared
recipe. Selection and publication approval remain explicit separate actions.
Use the native journal/profile status and recovery commands for admitted work;
preserve its original task, attempt, runtime and operation on uncertainty.

For an existing owned run, inspect its actual native frontier:

```powershell
reel-workflow-native inspect-run PLAN BINDINGS
```

After inspecting an interrupted handoff, reconcile its existing journal/profile
with `reel-workflow-native resume-handoff JOURNAL HANDOFF_PROFILE RETURNS_OR_DASH --apply`.
That command can perform lifecycle effects; retain the exact existing profile
and use the original return mapping or `-` as appropriate to its recorded state.
It is not a command to restart arbitrary failed work or replace its runtime.

## Release gates

The next release work is bounded by producer-visible outcomes:

1. Ship a reproducible CLI bundle, this entry guide and examples that run from a
   fresh output directory, with useful status/resume instructions.
2. Connect the existing local BBJ adapter to owned native jobs and demonstrate
   one actual issue through candidate production, review IDs, one correction,
   affected-consumer rebuild and retained editable/reading outputs.
3. Add an owned S5 adapter using existing media and corrected recordings;
   demonstrate its full bilingual episode plan and then execute within the
   actual resource, byte and time envelopes, including interruption/recovery.

BBJ's source format and larger issue closure still need integration. S5's
600-resource/~7.94 GB scale and multi-GB native transfer/reopen recovery remain
open; its larger streaming intent is not a working full episode dispatcher.
Actual provider/tracker credentials, creative approvals and profile adoption
are project-specific gates. These gaps should drive implementation and targeted
checks; further synthetic variants are not the release deliverable.
