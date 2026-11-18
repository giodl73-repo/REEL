# Native workflow input bridge

This bridge binds a task's exact Git commit, logical records and
explicit annex leaves using Tig's qualified Rust SDK. Its publication
`InputReader` holds verified native streams and executable handles through the
check. Explicit admission can now protect the declared input objects under an
existing enrolled job actor. A pinned staging ticket supports resumable private
input export. It does not publish
outputs, acknowledge retained ownership or apply selections.

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

Next: implement idempotent
registration/retained-owner recovery through `handoff::Lifecycle`. Completion,
selection and guarded scratch retirement remain separate. Project adoption
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
production, external candidate registration and retained-owner completion
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
staging, not result ingestion, retained ownership or production project adoption.
