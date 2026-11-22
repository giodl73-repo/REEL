# Protected native workflow and bounded film dispatch

This isolated adapter binds exact Git records to explicit annex leaves, admits
positive input claims, stages owned manual work, registers unselected responses,
reopens retained outputs and recovers the original operation. It also connects
W2 film metadata inspection to those native bindings. It calls no model/provider,
and selects no asset or changes any active profile. The optional Windows film
feature adds bounded owned production, retained candidates and explicit close.

It builds on [SDK intake PR #389](https://github.com/giodl73-repo/REEL/pull/389).
The separate workspace/lock preserves the existing producer SDK and root lock.
All original SDK/module source pins remain unchanged. The original slots module
and tests compile from byte-identical build-output copies in their required Rust
module layout; source tests check those copies and the unchanged vendor trees.

## Commands and identity

Run `reel-workflow-native --help` or `capabilities`. The entry points include:

```text
check-snapshot PACKET SNAPSHOT_PROFILE
check-native-inputs PACKET NATIVE_PROFILE
inspect-film PACKET NATIVE_PROFILE INSPECTION_REQUEST NEW_SIDECAR
preview-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT
accept-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT NEW_RECEIPT --apply
reopen-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT
prepare-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT STAGING_PROFILE NEW_TICKET
export-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE TICKET TICKET_SHA256
create-handoff PACKET HANDOFF_PROFILE NEW_JOURNAL --apply
import-handoff JOURNAL HANDOFF_PROFILE RESULT RETURNS --apply
resume-handoff JOURNAL HANDOFF_PROFILE RETURNS_OR_DASH --apply
preview-close CLOSE_REQUEST REQUEST_SHA256
stage-close CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply
close-retained CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply
recover-close CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply
```

Profiles are private caller-owned controls, never cache discovery. Snapshot
profiles pin Git, commit, record/definition scopes, every logical record's annex
binding and every explicit stream selection. Pin `declaration_bytes()` as the
task's `native_inputs` artifact. Machine locators are excluded from that authored
identity. Byte-identical alternate leaves do not replace the declared binding.
Read-only checks establish neither durable custody nor production eligibility.

Admission profiles name an already-enrolled effects-enabled Index, metadata
store, coordination root, actor/owner, original operation, store and policy.
They do not create a pool/store, enroll actors or grow limits. Preview observes
headroom; it reserves none. Acceptance preserves the original WorkRef, activation,
source archive and checkpoints. Waiting, timeout or cancellation does not release
claims. Source claims and guarded tool/file handles remain separate authorities.

A handoff profile binds the exact admission receipt, retained target, actual
pinned lifecycle executable and existing acknowledgement directory. The journal
records import intent before native registration. Logical registration identity
uses task/attempt; changed content or provenance conflicts. Uncertain work is
reconciled through its original handles, never a new random operation. Retained
completion requires native work, positive retained coverage and actual reopened
output bytes. Claimed provider provenance is not authenticated and native
retention grants no creative approval, selection or release.

`Returns` names an existing ordinary caller-owned root and a complete artifact
name -> portable relative file map. Returned files are verified without archive
extraction or execution. Owned staging uses exclusive files, preserved capacity
leases and bounded interrupted-copy recovery. Neither source nor scratch is
automatically removed. New private receipt destinations cannot overwrite evidence.

The strict `reel.workflow-native-close.v1` request contains `journal`, `packet`,
`source`, `admission`, `receipt`, `result`, `acknowledgement`, original close
`operation`, `original_lifecycle_tool` and separately pinned closer `runtime`.
Close requires the exact Retained journal and acknowledgement, reopens retained
bytes, and uses the original qualified slot protocol with ordered fences and
coverage checks. It hands coverage to a ready retained actor before clearing a
slot. A new closer does not retarget or rerun an admitted producer. Recovery uses
the SAME request, digest and operation, with a fresh receipt destination. Scratch
retirement, capacity release, payload deletion and retention release are absent.

## Film metadata and bounds

Native inspection supplies the [W2 adapter](../../docs/workflow-film-inspection.md)
only with controls present as exact artifacts in the task's native record/leaf
bindings. Request paths are portable control names in this adapter, not an
annex-leaf search. Project/commit and all artifact pins must agree. Its sidecar
still leaves media, renderer, approval and production adoption unverified.

Historical native v1 profiles retain their original 64 MiB aggregate limit and
unchanged declaration identities. New `reel.workflow-native-snapshot.v2` profiles
must supply a `budget` with schema `reel.workflow-native-input-budget.v1`, positive
`input_bytes` and `input_object_bytes`, up to a declared 64 GiB aggregate. Changing
that budget changes the task-bound declaration. The streaming reader keeps the
input context and native guards alive without a whole-payload buffer; the buffered
metadata reader stays limited to 64 MiB and directs larger reads to streaming.
The qualified single-store SDK and caller's native-reader bounds still apply.

These input descriptions do not expand executable workflow capacity. This slice
retains a 1 GiB protected envelope/retained aggregation scope, 128 MiB response
scope, 512 MiB staging scope, 512 resource objects, 128 changed paths per phase
and 16 total actors. Admission conservatively preflights response/close paths
before effects, counting unknown output hashes separately. Declared response
counts/bytes, the native reader's
per-object allowance and combined input/output envelope with metadata reserve
must fit before acceptance; incompatible declarations refuse before protected
work is accepted. Close/retention additionally
check actual target state. The 64 MiB legacy reader bound is not a universal film
limit. Large native metadata can be inspected while an unqualified large job is
refused before admission/production. Controls remain bounded to 1 MiB, with the
SDK's stricter 64 KiB metadata-node bound applying to persisted control nodes.

Composite graph jobs, large native episode execution, direct API/proxy calls and
new voice takes are not enabled. Optional film APIs provide bounded task-bound
production and retained candidates through an owned CLI workspace/capacity lifecycle.
Accepted existing film producers remain in use. Further W3a producer/scale work,
W3b providers, W4 review/rebuild and W5 owned adoption/rollback remain required;
this is not Season 5 adoption or full episode-scale qualification.

## Film file bridge (optional feature)

`--features film` adds a Windows-only `film_inputs::Inputs` implementation of
the existing renderer's `scene_delivery::InputFiles` trait. The default build
does not include the renderer. This bridge exposes exact guarded native paths;
it does not dispatch a producer or create producer completion receipts.

Construct `NativeInputs::from_claims` with a reopened live admission guard for
the exact packet/source. The bridge requires authored aliases covering every
declared input and the exact `FilmRender` contract. Matching bytes at another
logical input do not authorize an alias swap. Pinned `ffmpeg` and `ffprobe`
programs must match the task's auxiliary executable contracts; held installed
paths are returned directly, without ambient PATH lookup. Admission includes
all auxiliary runtime objects in the protected envelope. Reserved native path
names refuse before admission effects.

File access retains the claims, enrolled reader gates, verified provider files,
runtime handles and directory identities for the bridge lifetime. Only exact
declared FileRef names/hash/lengths resolve. Reads permit up to 128 inputs and
128 distinct live objects, a 64 GiB declared sum and a 30-second verification
budget; snapshot, provider and admission limits still apply independently.
These bounds are not episode-scale qualification or a new output/workspace/time
contract. The explicit existing-pool test covers one 1 MiB synthetic object,
alias deduplication, guarded path integrity and exact runtime selection; its
pinned stand-in programs are never executed as media tools. Actual FFmpeg
execution, conform receipts, large episode admission and retention remain open.

## Contained film process port (optional feature)

`film_process::{render,recover}` connects the live-claimed file bridge to the
existing scene renderer in a pinned child process. Its caller owns workspace
and capacity admission. The workspace must be new and empty for rendering;
recovery must match the exact original request, task key, input paths and runtime.
An existing launch is never issued again. Recovery uses a distinct independent
check launch, preserving the render and all original request/outcome files.
An uncertain or already-issued check remains a recovery case, not a fresh render.

The version-2 process outcome binds the complete output table, including the
delivery receipt. The parent reopens guarded files and compares that table before
returning them. `Rendered::files` contains held, verified file objects rather
than whole-movie buffers. `Output::copy_to` copies and hashes through a 64 KiB
buffer; it neither publishes bytes nor acknowledges retained ownership.

Explicit process limits allow up to five hours, 16 GiB of output and 64 GiB of
workspace. Memory, process count, frame/sample and raw-picture limits are also
declared and enforced. These ceilings do not enlarge native admission or prove
production-scale support. Ordinary tests stream a 128 MiB-plus synthetic file;
the explicit process fixture renders a three-frame synthetic scene, independently
checks it without replacing outputs, binds the child result and preserves failed
scratch. Neither fixture qualifies a multi-GiB master or real episode.

The internal `film-process-child ... --apply` command is a trusted process port,
not a workflow admission/completion command. The `capabilities` response reports
producer execution and owned dispatch only for the optional Windows film feature.
Episode-scale qualification remains false. The task API and owned CLI below add
bounded producer/retention steps. No profile, asset selection or delivery is adopted.

## Task-bound film production and retention (optional feature)

`film_task::bind` reads the exact task request, compiled job and production
controls. The request schema is `reel.workflow-film-task.v1`; it names the job
and production aliases, exact output-name/path map and explicit process limits.
All task inputs must have authored aliases, including the workflow request.
The three evidence outputs are required: `execution-request` at
`render-request.json`, `execution-result` at `render-result.json`, and
`delivery-receipt` at `delivery/receipt.json`. Other outputs live under
`delivery/`, with no case-folded aliases or file/directory collisions. The
existing producer's planned output set must match before rendering.

`film_task::run` consumes live native input authority for the original admission
receipt. It freezes the original invocation in the existing coordination root,
binding task/attempt, admission generation, source, runtime, media programs,
workspace path and filesystem identity. A second workspace for the same attempt
conflicts. Control-read gates are released before the path provider reopens those
objects; durable input claims remain live throughout that transition.

The returned `Generated` keeps actual outputs guarded. Its result includes a
canonical worker receipt and exact executable provenance. `Generated::stage`
records original registration work only; `ingest` protects and publishes the
unselected bytes, including the worker receipt. `retain` reopens them, positively
acknowledges retained ownership, then writes a durable technical completion
receipt. A JSON worker receipt alone cannot construct `Generated`; public
external-response APIs continue to refuse arbitrary tool responses.

`film_task::resume` reads the original result from native work evidence. It may
reconcile that original admitted response and, with the original target supplied,
retention work before reopening inputs. It independently checks original media;
it never renders. A completed check's exact request and output table can be
reconciled without another launch. Missing result controls may be reconstructed
from verified original outputs; partial or changed existing controls refuse.

`verify_retained` checks native work, the worker receipt and actual retained
bytes against the frozen task without accepting or rendering another job.
`close` explicitly stages/applies/recovers the original retained-owner handoff.
Persist its operation key before staging and reuse it on uncertainty. Close
reopens outputs before clearing the original slot; it does not retire scratch,
release retained history, select a candidate or approve a film.

New retention work uses the qualified close code's canonical `objects/<sha256>`
layout. An older retained tree may first require explicit
`prepare_close_layout` preview/application with the original close key. Its
reported add and cleanup phases each obey the 128-change limit: references are
added before aliases are removed, all prior objects and tree roots stay covered,
and original native work is reconciled on retry. This is not payload deletion or
an automatic workaround for oversized production admission.

The existing native bounds still apply: a 1 GiB protected envelope, at most 32
response objects including the worker receipt, and 128 MiB aggregate response
bytes. Admission reserves one 64 KiB worker object for tool tasks and includes it
in response/close headroom. Byte deduplication does not change logical result
identity. This is not S5-scale execution or owned-profile adoption; five-hour
process ceilings do not enlarge these native admission bounds.

An explicit existing-pool qualification fixture exercises a small synthetic
scene, interrupted native registration, lost result metadata, repeated recovery,
retained readback after source advance and original slot close. It requires a
new `REEL_NATIVE_FILM_EVIDENCE` directory, preserves all job/source/scratch roots
and freezes its worker/media binaries. It initializes no native pool and uses
no project assets. A failed qualification must continue those original handles,
not start a new case and assume the first one had no effects.

## Owned film CLI (optional Windows film feature)

The CLI consumes a private `reel.workflow-film-cli.v1` request containing the
exact packet, source/admission profiles, original accepted receipt, pinned
producer/media tools, retained target, original close operation key and an owned
workspace profile. Pin its raw SHA-256 externally. Profiles remain explicit
machine bindings; copy and pin one for your lane rather than editing another.

```text
reel-workflow-native prepare-film REQUEST REQUEST_SHA NEW_TICKET --apply
reel-workflow-native run-film REQUEST REQUEST_SHA TICKET TICKET_SHA --apply
reel-workflow-native resume-film REQUEST REQUEST_SHA TICKET TICKET_SHA --apply
reel-workflow-native verify-film REQUEST REQUEST_SHA TICKET TICKET_SHA
reel-workflow-native preview-film-close REQUEST REQUEST_SHA TICKET TICKET_SHA
reel-workflow-native stage-film-close REQUEST REQUEST_SHA TICKET TICKET_SHA --apply
reel-workflow-native recover-film-close REQUEST REQUEST_SHA TICKET TICKET_SHA --apply
```

`prepare-film` requires already accepted native protection. It validates the film
request and tool pins, reserves capacity using REEL's existing ledger, and creates
a fresh owned root with intent, internal ticket and empty `work` directory. The
ticket binds the request, admission identity, parent/root/work filesystem
identities, reservation, volume and byte budget. Persist the returned ticket and
its reported SHA before running. An internal ticket copy remains in the owned
root if external receipt delivery is lost; preparation never adopts an existing
directory or allocates a replacement on resume.

The workspace budget must cover the declared process workspace plus 4 MiB of
owner controls. Rendering borrows verified existing media through native paths;
the CLI does not export another copy of F-first media. `run-film` publishes and
retains the actual guarded candidate. `resume-film` uses original task/native
work and never renders again. Verification and close use the same original
ticket and close operation key; `close-film ... --apply` can also perform a full
explicit close without the separate staging command.

The exact capacity lease is held through each command and conservatively kept
after it. Close releases only the native job slot into retained coverage. It
does not release capacity, retire scratch, delete payloads or apply selection.
Wrong pins, changed tickets, replaced directories and an unavailable original
lease refuse. No command discovers or initializes a native pool.

This is the bounded native film path, not full S5/episode-scale qualification,
API/proxy generation, publication approval or owned project-profile adoption.
The 1 GiB protection/128 MiB response limits above still apply.

## Validation

```text
cargo test --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked
cargo clippy --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked -- -D warnings
```

Ordinary tests use synthetic Git metadata, isolated claims/metadata, the original
slot protocol tests, and owned scratch fixtures. Six explicitly ignored Windows
fixtures additionally require a caller-declared existing synthetic pool. They do
not set up or hydrate one. The new close fixture requires a new evidence directory
and preserves its original roots, request and pinned runtime. These explicit tests
exercise native input reads, CLI response replay, retained acknowledgement,
lost completion, staging and close/recovery. They are not real project acceptance.
The qualified SDK/manifest table is retained in [source pins](src/pins.rs).
