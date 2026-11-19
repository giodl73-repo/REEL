# Native workflow bridge

This bridge binds a task's exact Git commit, logical records and
explicit annex leaves using Tig's qualified Rust SDK. Its publication
`InputReader` holds verified native streams and executable handles through the
check. Explicit admission can now protect the declared input objects under an
existing enrolled job actor. A pinned staging ticket supports resumable private
input export. Read-only input checks, explicit response ingress and retained
acknowledgement commands have separate boundaries below.

Build/check this separate workspace explicitly:

```text
cargo test --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked
cargo clippy --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked -- -D warnings
reel-workflow-native check-snapshot PACKET SNAPSHOT_PROFILE
reel-workflow-native check-publication-native PACKET NATIVE_PROFILE
reel-workflow-native preview-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT
reel-workflow-native accept-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT NEW_RECEIPT --apply
reel-workflow-native reopen-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT
reel-workflow-native prepare-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT STAGING_PROFILE NEW_TICKET
reel-workflow-native export-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE TICKET TICKET_SHA256
reel-workflow-native advance-expanded PLAN FROZEN FROZEN_SHA256 CONFIG PRIVATE_DIRECTORY --apply
reel-workflow-native advance-recipe RECIPE BASE_PLAN BASE_BINDINGS CONFIG PRIVATE_DIRECTORY LIMIT --apply
```

The isolated graph consumes `vendor/tig-workflow`, preserving all 247 source pins
and the original source-pin manifest. It identifies dirty qualified source;
c62c58c ancestry alone is insufficient. The bridge lockfile was seeded from that
SDK lockfile to retain dependency versions. Existing producers still consume
`vendor/tig`. CI checks both workspaces explicitly. This is source integration,
not installation of a qualified lifecycle distribution or the newer four-store
reader. Upstream source bytes remain unchanged, including line endings.

`SnapshotProfile` has schema `reel.workflow-native-snapshot.v1`, pinned Git tool
(`path`, `sha256`, `bytes`), absolute repository, exact commit, explicit record
and definition scopes, complete `bindings` and `input_streams` maps. Every
logical input maps to its selected annex path and all declared stream paths.
Each input choice is explicit: null means selected; a string means that exact
slot. No alias inference, working-tree reads or latest-byte fallback occurs.

`SnapshotProfile::declaration_bytes()` produces canonical portable bytes. Pin
their SHA256, length and JSON media type in `Task.native_inputs`. The declaration
includes commit, scopes, record-to-leaf mappings and stream choices; private
machine tool/repository locators are excluded. Changing a byte-identical leaf
mapping requires a new task key. Preserve those authored bytes alongside the
original task definition. `handoff::Ticket.native_binding` is separate: that
operational admission must preserve accepted work, activation generation,
source/Index checkpoints and owner acknowledgement. Input identity is not job
admission. Existing v2 keys with no native declaration remain unchanged.

`NativeProfile` contains `snapshot`, pinned `annex` tool and the SDK reader's
explicit `source` (`pool`, `store`, `maximum_bytes`). Provider Git/annex paths
must match the pinned tools. This SDK supports one store. Native bytes require
qualified Windows scope; metadata checks also run on Linux. Only declared
inputs may be read. Callers must retain `NativeInputs` throughout their operation.

Bounds: 128 task inputs, 64 MiB declared input bytes, 256 scope paths, 512 metadata
documents / 16 MiB metadata, 1 MiB controls, 256 MiB per executable (stream-hashed)
and 256 read opens. Smaller caller/provider allowances apply. Typed SDK snapshot
errors are normalized; generic provider I/O remains conservative without parsing
error strings. Public diagnostics omit private paths and submitted text.

`check-snapshot` binds actual committed metadata without reading payloads.
`check-publication-native` additionally checks the comic closure using guarded
native reads. Both report durable job protection false. Their output is an
observation, not a lease; guards end when the command returns.

Seven real-Git tests cover historical commits, bare history with unusable original
checkout files, exact leaf/stream identity, declaration changes, tool pins and
CLI authority flags. Another test verifies all 247 SDK pins. An ignored Windows
test explicitly reads an existing synthetic store via `REEL_NATIVE_INPUT_FIXTURE`;
it creates no store or payload. It was run locally against a previously published
1 MiB object, including allowance and undeclared-read refusal. This does not
qualify a complete native comic job, external import or retained publication.

The native `handoff::Lifecycle` implementation below now connects idempotent
registration and retained-owner recovery to technical task completion.
Selection and guarded scratch retirement remain separate. Project adoption
requires those fixtures and an owned rollback.

## Explicit durable input claims

`admission::Prepared` archives the verified SDK source metadata and constructs a
bounded workflow input projection from the task packet, native declaration,
task/attempt descriptor, selected inputs, every declared annex stream and runtime
pins. This is a workflow adapter over existing Tig metadata/Resources/envelopes,
not a new cache or a claim of complete producer semantics. Source archives
restore records/definitions without authored working files. Payload availability
is independent and remains false in admission receipts.

The adapter imports the unchanged EVID-151 REEL slot module and original tests
under `vendor/reel-workflow-lifecycle`. Its fence/type shim uses the existing
REEL metadata lock and exact actor-lock domain. Both original module hashes are
verified. The included close implementation is not exposed by this adapter.

`reel.workflow-native-admission.v1` requires existing index/metadata/coordination
roots and trusted minimum checkpoints, exact index identity, actor/owner,
original operation, policy and store. The native source must declare that same
managed controller/store; unmanaged providers are refused. No enrollment,
bootstrap, source-owner selection or payload store is initialized by the adapter.
Operational store/controller enrollment and cooperating writers are explicit
trusted assumptions, not inferred from finding a directory.

Preview preserves metadata/index claims. Acceptance uses qualified typed-idle
admission, finishes/reopens metadata and restores the exact resource envelope,
then records original native work and activation. Reopening checks the original
WorkRef, completion, activation, task/attempt/context and current positive
coverage under the cooperating actor fence. A caller-supplied receipt is not a
guard. Other native actors may advance; the original generation must remain
valid. Unknown native/fence failures remain NeedsRecovery without string parsing.

The private receipt preserves native checkpoints and exact handles and can be
pinned as `Ticket.native_binding`. CLI acceptance requires `--apply` and reserves
a new private receipt outside index, metadata, coordination and native-pool
roots before effects. Failed/partial files remain. A lost reply is recovered
with the SAME task/attempt/operation/profile and a NEW receipt destination;
never allocate a fresh operation to bypass uncertainty. Reopening verifies real
native state, not JSON flags. No deadline or cancellation releases claims.

Admission has additional bounds: each canonical control fits one 64 KiB native
metadata node; native canonical JSON structural limits apply; declared resources
plus conservative envelope overhead fit the original 128 changed-path phase;
logical resources fit 1 GiB. The actor table has 16 TOTAL actors. Oversized work
is refused, not split or silently assigned another slot. Existing task-level and
reader bounds still apply. Inputs/runtime pins can be protected before bytes
exist; this is not availability, retention, custody, creative approval or task
completion. Claims may cover other stores only through their explicit enrolled
scope; this adapter admits one declared store.

Four original slot tests and six workflow/CLI admission fixtures exercise real
isolated native metadata journals, restart/lost-response reuse, claim coverage,
retained source metadata, scope refusal, node bounds and preserved CLI failures.
They do not create a native payload pool. Full native-pool adoption, comic
production and retained-owner completion
still require end-to-end qualification before a real film/comic project adopts
the workflow.

## Resumable private input export

`reel.workflow-staging.v1` declares an existing private scratch `parent`, a
`maximum_bytes` logical ceiling (at most 512 MiB), and the existing REEL
`capacity_policy`. The parent must be outside index, metadata, coordination and
native provider roots. It is scratch, not a new asset cache. The adapter reuses
REEL's shared capacity ledger under the original admission coordination root;
it never initializes a payload store or allocates another native actor.

Preparation reopens actual original claims, guards directory namespaces, reserves
capacity, creates a fresh operation-named workspace and immutable intent, then
returns a private ticket with parent/workspace file identities, exact intent pin,
original admission and reservation identity. Persist the ticket and pin its raw
SHA256 before exporting. The CLI requires a new private ticket file. An existing
workspace is not adopted by preparation. Initial failure before a complete
ticket leaves evidence and conservative capacity for explicit diagnosis; that
boundary is not automatically repaired or silently retried under another ID.

Export requires that external ticket pin. Each invocation reopens actual claims,
checks the same directory identities and intent, reacquires the original capacity
lease, verifies the workspace, and reads the original exact native inputs while
holding provider guards. Completed scratch copies cannot excuse a missing native
provider. There is no latest fallback, hydration, output import or provider call.

The bundle contains packet, original admission, native declaration, portable input
bindings and the exact prompt when present. Generic inputs use numbered paths;
comic source bindings preserve a separate `source/request.json` and exact
producer-relative files. Comic source/edition correspondence is checked before
export. No native comic render or multilingual creative approval follows.

Files are exclusive-create immutable parts, flushed before hard-link publication
inside the owned workspace. Resume verifies/reuses a complete part or creates a
new bounded part; incomplete parts remain untouched. A completion marker is
committed only after input verification and is rechecked against actual files.
`already_complete` means the verified marker existed at the start. Unexpected
entries, altered files, replaced directories, wrong tickets or exhausted bounds
stop without cleanup or replacement. Outputs belong outside this immutable input
bundle until a separate output-admission adapter is qualified.

Bounds are 256 exported files, 2,048 workspace entries, 16 path segments, four
part attempts per file, and the existing 64 MiB input/1 MiB control bounds.
Logical scratch usage includes parts AND published names, even for hard links.
Capacity/free-space policy is checked and preserved; neither a completed export,
timeout nor failure releases its record. Separate retained-result verification
and guarded retirement must precede explicit capacity release. Directory/file
guards protect active operations; durable native claims survive process exit.
Power-loss directory-entry durability and hostile local control rewrites are
outside this qualification.

Five filesystem fault fixtures and one CLI preflight fixture cover partial and
flushed copies, no-overwrite recovery, ownership, tampering, bounds and missing
sources. An explicit Windows fixture combines actual admitted native claims,
guarded access to an existing synthetic 1 MiB payload, preserved capacity and
CLI restart/export. Its private controller enrollment/cooperating-writer scope
is a declared fixture assumption; it changes no provider defaults or project
bindings and does not attest all external writers. This qualifies bounded input
staging, not retained ownership or production project adoption. Response ingress
has its own additional qualification below.

## External response registration and ingress

The `response` adapter accepts the exact workflow packet, original input
admission receipt, returned result and explicit `reel.workflow-returns.v1` file
map. The return directory must already exist outside managed roots. For example:

```json
{"schema":"reel.workflow-returns.v1","root":"F:/private/returns/attempt-1","files":{"note":"note.txt"}}
```

`stage-response ... --apply` verifies all files and creates immutable native
work under one logical task/attempt key. It does NOT admit output protection.
`register-response ... --apply` additionally executes that original work and
verifies positive job protection. `ingest-response ... --apply` also publishes
each exact object through the pinned Rust native publisher and retains all
publication handles until every object has reopened. Each command takes
`PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT RETURNS NEW_CANDIDATE`.
The candidate file is private, new and outside managed roots. A deserialized
candidate or an old CLI success report is not current byte/ownership proof.

Logical identity includes index, task key and attempt, not output hash, supplied
provider identity, activation or file location. Its immutable evidence binds the
whole returned artifact set, provenance claims, original admission and store.
Identical retries reuse the exact WorkRef; changed bytes, logical IDs, provenance
or source generation conflict. Content deduplication cannot bypass this check.
All files are hash/length checked before staging; guarded files and directories
remain held throughout the call. Paths are explicit portable relative names,
case aliases are refused and no browser package is extracted or executed.

Registration extends the original actor envelope, preserving its original native
selection, policy, activation and every current reference/root. It uses existing
SDK metadata/work/index transactions and the original actor fence, not a new
cache or generic JobContext. Outputs are positively protected BEFORE native
publication. One atomic update must fit the native 128 changed-path bound;
there is no automatic splitting, new slot or substitution. An interrupted native
Prepare can reconcile ONLY the original matching admitted WorkRef before settled
claim verification; changed-result recovery refuses without reconciling it.

On uncertainty, preserve returned files and retry the original result/attempt
with a new candidate receipt destination. All files are reverified, even when
native bytes already exist. Explicitly resupplying identical bytes at a new
declared return location is allowed. Missing/corrupt returns stop; no fallback
or fresh operation is inferred. Ingress may have succeeded before receipt writing
fails. This is why failed files and original operation identities are preserved.

Bounds: Windows/NTFS, one explicitly enrolled existing native store, up to 256
declared returned artifacts plus a tool receipt, 128 MiB total payload including
that receipt, one 64 KiB registration metadata node, and existing native phase
bounds. The job's resulting flat reference tree must fit the qualified 128-path
close phase as well as each update's changed-path limit. Oversized imports refuse
before protection/publication; issue partitioning is explicit planning work.
Direct/manual provider and deterministic comic results are accepted. Comic
receipt/source replay has its own additional requirements below. MIME declarations
and supplied provenance are checked as contracts, not authenticated or creatively
approved. Opaque browser payloads receive byte validation only.

Three metadata/file fixtures cover original logical replay, logical-ID/provenance
conflicts, unsafe/incomplete mappings, changed bytes and fault-injected native
Prepare recovery. One explicit fixture ingests a synthetic response into the
existing test store, reopens its exact bytes, retries in another CLI process and
verifies the same WorkRef and original positive input/output claims. It initializes
no pool, changes no defaults and selects no project asset. Retained-owner ack has
its own adapter below. Handoff coordinator completion, capacity release, scratch
retirement, actor close and selection remain separate unimplemented operations.

## Retained actor acknowledgement

`retention` adds workflow-specific retained actor registration over the existing
SDK work/index/metadata primitives. It does not impersonate a producer
`JobContext` or the managed scene runner's SourceJournal outcome-adoption schema.
It requires an already registered response and exact task/attempt/admission:

```json
{"schema":"reel.workflow-retained-target.v1","actor":"retained","owner":"owner"}
```

`stage-retention PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT TARGET
NEW_ACK --apply` freezes an immutable retained update but grants no new retained
claims. `retain-response` takes the same arguments, reopens the already published
outputs, executes the original retained work and verifies positive retained-owner
coverage. `verify-retained PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT
ACK` rechecks actual original work, metadata/checkpoint witnesses, ready retained
coverage and exact native output bytes. It writes nothing. All receipt files
remain private; their JSON flags/descriptors are not authority. A live
`RetainedGuard` holds the retained fence, exact snapshot/runtime pins and native
output guards. No command marks the handoff task complete or retires scratch.

The ready retained actor must differ from the job and routed source actors, have
the exact enrolled owner and cover EVERY store assigned to the job actor. New
admission takes source/retained actor fences in stable order. The update preserves
all retained paths and its prior root, adds every current job reference/root and
pins immutable registration evidence. Existing history is never replaced or
deleted. It is bounded to 512 distinct resource objects, 1 GiB summed logical
bytes, one 64 KiB evidence node, 128 changed paths and the existing tree limits.
It refuses oversized work rather than splitting phases or allocating a new actor.

Logical retained identity is index/task/attempt; immutable evidence binds the
original response WorkRef, result registration, admission and target owner. A
changed target/result conflicts. Interrupted native Prepare is reconciled only
through that original admitted work; failed replies use a NEW receipt destination
with the same inputs. An already completed operation can be rediscovered and
its acknowledgement regenerated after the original source actor advances.
Checkpoint witnesses may advance; the original WorkRef and evidence remain exact.

Historical verification consults immutable original response/retained work and
current retained coverage. It takes only the retained actor fence, so another
job using the former source actor does not block it. Missing native bytes or
lost retained coverage refuses; neither source scratch nor newer candidates can
substitute. Only result output bytes are reopened: historical input claims and
archived metadata do not assert every input payload is currently available.
The current byte reader still uses one explicit existing store; other assigned
stores must be covered by enrollment/positive retained claims, not discovered.

Two metadata fixtures cover staged logical replay, wrong owners/actors, incomplete
store coverage and original native Prepare recovery without releasing job claims.
An explicit native fixture qualifies exact retained output reopening, uncertain
update recovery across CLI processes, preserved unrelated history, historical
readback/ack regeneration after source advance and refusal after lost coverage.
Its source advance and loss/restoration of coverage are explicit synthetic SDK
test operations, not qualification of slot close or retirement. No payloads are
deleted. The native `handoff::Lifecycle` port and technical task completion are
described below. Expected-baseline selection,
guarded actor close and scratch retirement remain open. Native byte/claim verification grants no creative,
translation, rights, print or release approval.

## Native handoff completion journal

The `handoff` adapter implements the existing `reel-workflow::handoff::Lifecycle`
port over real native response/retention work. It uses the existing append-only
journal. `create-handoff` verifies original live input protection before writing
`Waiting`. Import flushes `ImportPending` before ingestion. Only a verified
retained acknowledgement and reopened exact output bytes permit the final
`Retained` event and `task_completed:true`. This is technical task completion;
creative approval and selection remain separate.

Its private `reel.workflow-native-handoff.v1` profile contains:

| Field | Meaning |
| --- | --- |
| `source` | Existing `NativeProfile`, exact committed input binding and pinned Git/annex tools. |
| `admission` | Existing input-admission controller profile and trusted minima. |
| `binding` | `reel.workflow-native-handoff-binding.v1`, original `admission` receipt and exact retained `target`. Its canonical artifact is pinned by the journal ticket. |
| `runtime` | Exact Tool pin for the executable actually running this adapter. A different named executable cannot satisfy it. |
| `acknowledgements` | Existing private control directory outside managed roots and the journal. Machine locator only. |

```text
reel-workflow-native create-handoff PACKET HANDOFF_PROFILE NEW_JOURNAL --apply
reel-workflow-native import-handoff JOURNAL HANDOFF_PROFILE RESULT RETURNS --apply
reel-workflow-native resume-handoff JOURNAL HANDOFF_PROFILE RETURNS_OR_DASH --apply
```

`RETURNS` is the explicit return-file map used by ingress. `-` supplies none: a
still-unretained response requires those files, while an already staged retention
operation or retained result is reconciled through original native handles.
Completed-journal resume verifies current retained claims and native bytes; a
saved success report or plain local `check-handoff` inspection is not proof.
Journal/runtime/target binding changes refuse before import intent. Direct/manual
provider results and deterministic comic results are supported. Comic tool results
require the receipt/source/replay checks below.

Reopening is passive. During recovery, a separate original-generation fence may
coexist with native Prepare; it grants no mutation capability. Effect entrypoints
reacquire/recheck the original generation and reconcile exact original work.
Durable input claims remain throughout. Native read handles hold object gates,
so new retained admission runs after the initial byte read handles close, under
original positive job protection and ordered source/retained fences. Lost
original response coverage refuses before new admission, including retries of
merely staged work. Outputs reopen after admission; that proof remains held
through completion append.

Acknowledgement controls are canonical, exclusive-create, flushed and reopened.
An existing file must match original candidate/work/evidence/target and pass
native verification; fresh checkpoint witnesses do not overwrite it. The journal
pins its exact artifact. Lost completion reuses that proof without re-importing
bytes. Partial/corrupt controls and event tails remain untouched and stop recovery.
Initial uncertainty before a complete Waiting record or acknowledgement requires
explicit diagnosis. Journal/acknowledgement writers use existing cooperating
metadata fences; active namespace/file/runtime guards protect each call. Long
waits retain native claims rather than OS handles. No deadline or cancellation
releases claims.

The acknowledgement directory is bounded to 128 ordinary files of at most 64 KiB
each; journal limits remain 128 events/1 MiB per event. These control directories
must stay outside native pool/index/metadata/coordination roots and not overlap
each other. Existing actor, changed-path, byte and resource limits still apply.
Exact frozen executables must be preserved for old tickets; runtime upgrades
need explicit migration. No store initialization or additional cache is added.

Default fixtures cover runtime/target/control binding, unavailable providers and
preserved intent/claims. A native fixture combines actual input claims, response
Prepare recovery, publication, NEW retained admission, byte reopening and journal
completion across CLI processes. It also tests lost completion, resume without
return files, a busy former source actor and corrupt-proof refusal. It exposed
and fixed the read-gate/new-admission conflict that retained recovery alone had
not covered. Provider execution, complete comic production, selection, guarded
actor close and scratch/capacity retirement remain separate acceptance gates.
No production project or public release is adopted by these fixtures.

## Native comic worker receipts and replay

The v2 result envelope has an optional `worker_receipt` artifact. It is omitted
when absent so existing provider result bytes remain stable. Legacy local tool
checks may still supply a separate receipt; native comic import requires the
pin, JSON media type, bounded length and distinct logical identity. The bytes
live at `receipt.json` beneath the explicit returned packet root. They are part
of original logical registration, native publication and retained proof, rather
than mutable side evidence outside task identity.

The same frozen native executable supplies `comic-render REQUEST NEW_DIRECTORY`
and `comic-check REQUEST DIRECTORY`, using the shared publication renderer. These
standalone producer/check commands grant no retained ownership or task completion.
Use a worker/verifier pair with matching frozen renderer dependencies; version
differences that change bytes require an explicit new derivative, not acceptance
of whichever output happens to exist. Executable provenance remains a supplied
claim; deterministic replay verifies content without authenticating a worker.

Before native registration, the adapter resolves every declared comic input
against its exact Git commit/record/annex binding, checks the selected edition,
language, page/panel/utterance order and full source map, verifies the pinned
receipt and refuses worker approval/retention/print authority. It then replays
the actual renderer against those native sources and compares every render,
transcript, PDF, receipt and captured editable source copy. Returned directories
must have exactly the declared packet files, ordinary namespaces, at most 1,024
entries and depth 16. No arbitrary archive extraction or returned code execution
is involved. Active file/directory guards preserve the checked files during ingest.

Retained verification repeats deterministic replay against exact native source
and output objects. It no longer needs the original worker source directory or
return files, but DOES need the retained source payloads as well as outputs;
missing historical source bytes refuse without a current/latest fallback.
Captured source copies are reconstructed from those original pinned native
inputs for replay. Explicit packet export below uses that same verified closure.

Native SDK object gates are exclusive. The adapter shares one held guard per
physical object inside an operation, including duplicate output bytes and inputs
whose exact declared records resolve to the same object. Logical record/leaf
validation occurs before any reuse. This is bounded in-memory verified-read reuse,
not a persistent cache, lookup substitution or logical registration dedup engine.
Read gates close before new index admission and reopen for retained proof. Existing
256-read/64 MiB input and renderer page/pixel/byte limits continue to apply.

A real synthetic fixture publishes declared sources into the existing test pool,
runs the pinned native comic worker, rejects forged approval and altered render
bytes before registration, then completes the native journal. It switches to
bare Git history and removes the original worker input directory before retained
resume. It grants no project/creator/font/printer approval. The provider ingress
fixture also registers two logical outputs with identical bytes to check guard
sharing without losing logical identity. A separate fixture refuses output sets
that would strand a job beyond the qualified close bound. Larger issue planning,
native issue scheduling, provider execution, selection and retirement remain
open in the [full completion ledger](../../docs/workflow-completion.md).

## Explicit comic part tasks

An optional `tool.part` on `reel.workflow-tool.v1` has schema
`reel.workflow-comic-part.v1`, an exact `plan` Artifact and a `part_id`.
The plan must be a distinct declared JSON input, bounded to 1 MiB. Existing tool
tasks omit this field when absent, preserving their original serialization and
fingerprints. It is not a prompt, an implicitly selected record or a new provider.

The binding reads the full master publication and pinned issue plan, validates
all contiguous groups/spreads, and requires the embedded master and edition to
match exactly. It then reads only the selected group's page/font/art/request
closure, checking original logical page identities, panel/utterance order and
language. Each part still uses the existing producer limits and all native
source/phase/close/actor admission bounds. Part output contracts are the task's
frozen five-per-page derivatives plus its own PDF/transcript; the master's final
edition contracts remain unchanged. Parts' PDF/transcript logical IDs are scoped
by their separate task/attempt context and must not be treated as the final issue.

Master, plan or part changes invalidate the task key. Whole-master changes
currently invalidate all bound parts, even if a local page's bytes did not change.
Narrower cross-commit reuse requires an explicit checked recipe-equivalence path;
matching bytes alone cannot replace source/governance identity. The part selector
does not schedule other parts, allocate actors, select results or complete an
issue. Native final assembly still needs explicit retained upstream dependencies.

## Retained comic packet export and editable restoration

```text
reel-workflow-native prepare-comic-export PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT ACK STAGING_PROFILE NEW_TICKET --apply
reel-workflow-native export-comic PACKET NATIVE_PROFILE ADMISSION_PROFILE TICKET TICKET_SHA256 --apply
reel-workflow-native comic-restore PACKET NEW_DIRECTORY RECEIPT_SHA256
```

Preparation verifies the original native retained work, actor/store coverage,
source snapshot/record/annex bindings and actual source/output bytes. Comic replay
captures the exact producer packet, including editable source copies. Provider
results cannot masquerade as comic packets. Preparation then uses the existing
staging profile (`reel.workflow-staging.v1`), owned-directory intent and preserved
capacity admission. Persist the new private ticket and pin its exact raw SHA256
before exporting. Failure during initial preparation or ticket writing preserves
scratch and conservative capacity; diagnose the original operation rather than
allocating another identity.

Export reopens the exact ticket and its original acknowledgement, verifies native
retained proof again, and holds source/output reads and retained actor fences
through immutable file publication and final verification. It reuses the staging
engine's bounded attempts, flushed part files, exclusive hard links, namespace
identities and conservative capacity lease. Interrupted writes preserve evidence;
matching flushed parts can resume. Changed files, replaced directories, unexpected
entries and exhausted attempts refuse without overwriting or deleting anything.
The original input-staging ticket/completion schemas and bytes remain unchanged;
comic exports have separate operation, ticket, manifest and completion schemas.
Export operation identity derives from the original admission identity and
retained WorkRef, excluding checkpoint witnesses that may advance on rediscovery.
Exact original controls remain bound by the ticket/intent. A refreshed witness
cannot silently create a second workspace or replace the original pinned ticket.

`comic_export::Ticket::packet_directory()` provides the private producer-packet
locator. The workspace contains `packet/` (the exact render outputs, worker
receipt and `sources/` tree), workflow controls and owned intent/completion files.
The `packet/` subtree can be checked or restored using its externally pinned
worker receipt. The source-restoration command alone validates a local packet's
source hashes; it does not recheck native custody. Restoring sources supports an
exact rebuild and a fresh local lettering version with clean art preserved.
It does not restore a whole Git repository or select the edited version.

Every export call rechecks native proof, including an already completed export.
Missing native bytes or history cannot fall back to the copied packet or its
completion marker. Scratch must remain outside native pool/index/metadata/control
roots and the source Git repository. Existing staging limits still apply: 256
files, portable names/depth, 512 MiB maximum reservation with copy/evidence budget,
four immutable part attempts and 2,048 workspace entries. Comic renderer limits
remain smaller where applicable; oversized exports refuse instead of silently
splitting work. The complete export packet and all controls remain private.

This operation acknowledges no new task completion, releases no source claims,
does not retire scratch/capacity, and does not change asset selections. An exported
packet can supply an explicit local issue-assembly part; native upstream-result
dependencies, issue scheduling and final issue retention remain separate work.

## Verified frontier and manual ready batches

```text
reel-workflow-native inspect-run PLAN RUN_BINDINGS
reel-workflow-native export-ready PLAN RUN_BINDINGS TASK_IDS_JSON NEW_PRIVATE_BATCH --apply
reel-workflow-native import-ready PLAN RUN_BINDINGS BATCH BATCH_SHA256 TASK_ID RESULT RETURNS --apply
```

The existing workflow plan remains `reel.workflow-plan.v2`. Private
`reel.workflow-run-bindings.v1` controls map task IDs to existing journal paths,
an exact profile file pin (`{path, sha256}`), and an optional staging ticket file
pin. Missing entries mean explicitly unadmitted work; a configured journal/profile
that is missing or corrupt refuses rather than being treated as a fresh job.
The inspector reads only named controls, not a workspace-wide job census.

The frontier checks tasks in topological order. A retained result unlocks children
only after its original packet/attempt/result/acknowledgement match and native
retained work, coverage and bytes verify. Children bind the actual canonical
result digest and artifact set. Changed plans, attempts or parent results make old
child packets stale. Independent branches remain inspectable after a refusal.
Waiting jobs recheck original claims and target readiness; pending imports require
recovery, cancelled jobs remain cancelled, and neither is dispatched automatically.
Inspection prints task/key/status metadata and sanitized errors, never prompt text.
These are sequential observations, not an atomic custody snapshot or a new run
completion certificate. The JSON says `all_tasks_retained_observed`, not that a
new owner has accepted a complete production.

Export requires an explicit nonempty task-ID list, at most 16 entries, with no
implicit truncation or actor allocation. Every selected job must already be
admitted and Waiting with its own exact staging ticket. Export locks the original
journal controls, reopens original claims, and uses existing owned staging to
verify/materialize all named native input bytes before writing one exclusive
private `reel.workflow-ready-batch.v1` control. It preserves original tickets,
runtime pins, attempts, prompts, packet/dependency metadata and input-binding
locations. A claim or local completion file cannot replace missing native bytes.
Existing/unsafe batch paths refuse; interrupted writes and conservative capacity
reservations remain for diagnosis. Batch paths cannot pollute source journals,
acknowledgement directories, source repositories or managed/staged input roots.
Shared acknowledgement directories do not require taking the same writer fence
twice during a multi-job export.

The exported task's `packet.operation` distinguishes intelligence prompts from
deterministic tools. Export invokes neither. Operators can run the frozen prompt
and input package externally, then return a normal result/returns manifest.
`import-ready` pins the exact batch, rechecks the current plan/frontier and original
job binding, and delegates to the existing lifecycle. Changed plans/parent contexts,
attempts, profiles or replay bodies refuse before effects. Matching pending or
already retained results reconcile the original operation, without a replacement
attempt. Effects still require the ORIGINAL pinned lifecycle executable. A newer
read-only inspector may observe an older qualified runtime; it cannot impersonate
that runtime for import. Use its existing `import-handoff`/`resume-handoff` interface
when handling older tickets that predate these graph commands.

Dependency artifact references remain planning metadata. This does not forward
unselected parent payloads as child inputs or silently register them as Git records.
Current byte inputs must still resolve through each task's explicit committed
native declarations. Retained upstream input adapters, complete comic assembly,
general worker/provider dispatch, actor admission/close scheduling and resource
retirement remain in the full completion ledger. Batch controls are private
instructions/evidence, not reusable proof of current custody or release permission.

## Owned deterministic comic worker

```text
reel-workflow-native prepare-worker PLAN RUN_BINDINGS BATCH BATCH_SHA256 TASK_ID STAGING_PROFILE NEW_TICKET --apply
reel-workflow-native materialize-worker PLAN RUN_BINDINGS WORKER_TICKET TICKET_SHA256 --apply
reel-workflow-native run-worker PLAN RUN_BINDINGS WORKER_TICKET TICKET_SHA256 --apply
```

One explicitly selected, already protected ready task can now use the in-process
Rust comic producer. Its tool contract must name the exact running native
executable; another runtime or operation refuses. This path executes the known
comic implementation, without interpreting prompts as commands or invoking a
provider. Both whole editions and explicit selected issue parts use the existing
exact source binder and producer limits.

Preparation rechecks the current plan, original batch/profile/attempt, source
claims and actual native inputs. It reserves an owned workspace under an existing
staging parent outside source/control roots. The staging budget must cover twice
the request's output limit plus 10 MiB of controls/evidence. The private ticket is
written exclusively and must subsequently be supplied with its exact byte hash.
An interrupted preparation preserves its partial control and conservative capacity.

Materialization creates a complete editable producer packet plus pinned invocation,
result, return and inventory controls using the existing exclusive staging engine.
An initially Waiting task remains Waiting. `run-worker` materializes if needed,
verifies the complete owned packet, constructs the normal tool result, and delegates to the
original native import/retention lifecycle. Source read gates are dropped before
new lifecycle effects; durable source claims remain. Completion requires verified
retained ownership. A local complete marker or worker manifest cannot confer it.

Retries use the same ticket, task, attempt and operation. Interrupted deterministic
writes reconcile identical files through the bounded staging mechanism. Pending
or retained native effects reopen the existing completed packet and reconcile the
original handoff result. Changed output, unknown files, replaced ownership, stale
plans or mismatched runtime pins refuse without overwriting evidence or starting
a fresh attempt. Scratch, capacity and actor slots remain preserved. No asset
selection, creative/translation/font approval, printer readiness or release follows
from technical completion. General workers, issue assembly from retained parents,
provider execution and admission/retirement scheduling remain open. Renderer
limits do not guarantee native result admission: the actual distinct resources
must still fit the existing close bound. Explicitly partition larger editions;
materialized outputs and original claims remain preserved if result admission
refuses.

## Retained dependency input reads

The additive task `upstream_inputs` pin identifies exact canonical
`reel.workflow-upstream-inputs.v1` controls. Each input logical identity maps to
a parent task and an explicit `output { name }` or `worker-receipt` selector.
The parent packet/result must match the child's project, source commit,
dependency result digest and complete artifact set. Identical payload bytes do
not permit changing the parent, output, attempt or logical identity. Changing
the binding pin changes the child task key. Absent pins preserve existing v2
packet bytes and fingerprints.

The library `upstream::BorrowedInputs` implements `InputReader`. Opening it
verifies actual original native work, retained-owner coverage, complete result
bytes and comic replay for every named parent. Parents sharing an exact
controller/retained actor reuse one live fence. Full parent proofs release their
object gates before another proof opens, and selected input reads then share a
bounded physical byte cache only after each logical binding has been checked.
Tool pins and retained-owner fences outlive selected read gates.

The current reader requires one exact declared native reader configuration for
the batch. There is no implicit multi-pool lookup, latest fallback, Git record
registration, external upload or selection. Named input bytes are bounded to
64 MiB, parent packet/result evidence to 32 MiB and read calls to 256. Binding
controls remain within the existing 1 MiB control limit.

This is live borrowed custody. It is **not durable child admission**, waiting
protection, task completion or permission to release parent history. Git-only
source profiles refuse upstream-bound tasks; the explicit composite path below
creates child-owned protection. No worker command silently bypasses source
admission. Comic assembly must
still use its master/part plan for reading order; input-map ordering is not page
ordering.

## Composite child sources and durable admission

```text
reel-workflow-native preview-admission PACKET SOURCE ADMISSION ATTEMPT
reel-workflow-native accept-upstream PACKET SOURCE ADMISSION ATTEMPT PARENTS NEW_RECEIPT --apply
reel-workflow-native recover-admission PACKET SOURCE ADMISSION ATTEMPT NEW_RECEIPT --apply
```

`reel.workflow-native-composite-snapshot.v1` adds a frozen upstream origin set to
the snapshot profile. Its separately pinned `reel.workflow-native-composite-bindings.v1`
declaration includes the Git scopes and portable parent packets/results/bindings.
The source archive stores exact Git metadata, binding and parent evidence objects,
and every declared input object. Git-only and upstream inputs are disjoint by
logical identity. Both mixed sources and upstream-only sources require an exact
declared Git commit; the latter use empty record/definition/binding scopes.

Frozen metadata alone cannot grant first acceptance or unguarded composite byte
reads. `upstream::Admission` verifies live retained parents, exact origin metadata
and the shared store/controller/metadata/coordinator binding. Parent owner fences
and actual tool pins remain live until the child accepts its own bounded closure.
An explicitly enrolled separate job actor is required. Existing actor and atomic
claim/close limits apply; this path allocates no actor and splits no transition.
CLI parent controls use `reel.workflow-native-parents.v1` with explicit packet,
result, handoff profile and retained acknowledgement per parent.
Git input byte availability remains a subsequent protected-read/staging check;
admission protects declared resources and does not certify all mixed payloads
as materialized or renderable.

After acceptance, `NativeInputs::open_admitted` or `from_claims` verifies original
child work, activation and coverage. Staging exports use that guard. Child input
reads and stage resumption use the frozen archive plus the child's claims, without
reopening parent profiles or requiring parent coverage to remain unchanged.
Only explicitly declared bytes and origin evidence become child resources;
undeclared parent source payloads are not inherited. Parent history release and
payload deletion remain separately authorized operations.

If receipt recording is lost after completed acceptance, `recover-admission`
reconstructs and verifies the original completed work/activation under a live
child fence and writes a new exclusive private receipt. It never creates an
acceptance, changes attempt identity or substitutes latest inputs. Pending,
uncertain, idle or reused states refuse for explicit original-work recovery;
they do not release claims. Partial receipt files remain preserved.

The existing manual result/retention path can carry composite intelligence tasks.
Composite comic workers use child job claims for generation and returned-file
replay. Historical native replay/export uses sealed permits derived from actual
retained coverage, so it does not require the original job actor to remain active.
Permits bind the exact input selection and source declaration; they cannot be
deserialized. Shared fence ownership allows nested replay without acquiring an
already held actor lock, and source byte gates still drop before index effects.
The [complete-issue adapter](../../docs/handoff/2026-11-18-native-comic-issue.md)
adds explicit `comic-issue-assembly` dispatch and full editable packet replay;
its native end-to-end qualification is recorded separately. Newly authored part
requests can set `part_id` for distinct part-scoped output identities. Automatic
derived-control production, larger issue scheduling, guarded slot closure and
scratch/capacity retirement remain separate work. Input admission
or technical completion does not select an asset or grant creative/release approval.

## Late-bound task planning

`expand-plan PLAN RUN_BINDINGS TEMPLATE NEW_PRIVATE_EXPANSION --apply` fills a
new task's input and tool roles from exact verified retained parent selectors.
It exports a bounded proposed plan, task packet and upstream bindings while
preserving existing tasks. The [task expansion contract](../../docs/handoff/2026-11-18-task-expansion.md)
records positive native history verification and its explicit planning scope.
The controller must still freeze the new source declaration and admit the child;
expansion does not dispatch a provider, allocate an actor or grant custody.

`prepare-expanded` freezes that task's exact configured source, original parent
controls and admission preview into a new pinned package. `accept-expanded` and
`recover-expanded` require its SHA256 and the exact base plan before using the
existing qualified admission/recovery interfaces. The [frozen expansion contract](../../docs/handoff/2026-11-18-frozen-expansion.md)
records the positive composite case through native staging and a protected
waiting prompt batch. No provider execution or task completion follows from it.
