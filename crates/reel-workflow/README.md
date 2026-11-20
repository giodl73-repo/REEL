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

## Run protected work

For an existing BBJ book, the [comic publication guide](../reel-publication/README.md)
provides `bbj-prepare` and `layered-compose-book`: actual selected SVG sources are
composed and compared, exact reading PNGs are reused, and a private reader plus
layer/source bundles are written with stable review IDs. This local adapter is
not yet wired into native workflow admission or the issue/fix task graph.

The same operations are available from the main workflow CLI built with `comic`:

```powershell
reel-workflow bbj-prepare $selection F:/src/bbj my-issue en F:/work/my-issue-prepared
reel-workflow bbj-compose F:/work/my-issue-prepared F:/work/my-issue-review
```

Open the resulting `index.html`. Use a new directory; partial outputs are retained
on failure. `bbj-compose-page REQUEST INPUT_BINDINGS NEW_DIRECTORY` composes one
candidate SVG separately. These commands are local publication operations;
the native workflow lifecycle below governs protected production jobs.

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
2. Add an owned BBJ adapter and demonstrate one actual issue through planning,
   candidate production, review IDs, one correction, affected-consumer rebuild
   and retained editable/reading outputs.
3. Add an owned S5 adapter using existing media and corrected recordings;
   demonstrate its full bilingual episode plan and then execute within the
   actual resource, byte and time envelopes, including interruption/recovery.

BBJ's source format and larger issue closure still need integration. S5's
600-resource/~7.94 GB scale and multi-GB native transfer/reopen recovery remain
open; its larger streaming intent is not a working full episode dispatcher.
Actual provider/tracker credentials, creative approvals and profile adoption
are project-specific gates. These gaps should drive implementation and targeted
checks; further synthetic variants are not the release deliverable.
