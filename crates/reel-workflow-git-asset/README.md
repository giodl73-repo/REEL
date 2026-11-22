# Public git-asset workflow adapter (experimental)

An isolated Rust integration for REEL's `publication::InputReader`. It pins
the candidate public git-asset at `0ccb137cf5a78416e780f8a3372017f642a5cb71`, with its own
Cargo.lock. It preserves the existing private vendor and native workflow route.
No existing job is upgraded or rebound by adding this package. This candidate
pin is under source/consumer qualification; do not activate a production route
from this update.

1. Create `git_asset::source::Repository` with an explicit Git executable,
   repository and metadata listing/command limits.
2. Call `Snapshot::capture` with a full immutable commit ID and explicit
   record/definition scopes. References such as HEAD/main are rejected.
3. Call `Snapshot::preview` with exact REEL Artifact descriptors, a selector
   for every declared ID, and a logical-byte budget. `None` means the selection
   in that captured commit; `Some(slot)` means exactly that stream.
4. Retain the captured Snapshot. `Preview::decode` accepts at most 128 KiB;
   `Snapshot::verify_preview` rebinds all decoded fields to captured Git bytes.
5. `GuardedInputs::new(profile, &snapshot, &preview, budget)` revalidates the
   preview, then creates a pure managed reader. Only `InputReader::read` opens
   a named payload. Each opened byte/namespace guard stays held until the reader
   is dropped. Bytes are checked against both declared hash and length.

A preview lists metadata pins and deduplicates neither logical IDs nor logical
byte totals. Duplicate IDs are refused. Equal payload hashes may serve separate
logical inputs. This is the closure explicitly declared by REEL, not a graph
inference of semantic dependencies. Metadata listing is bounded across the Git
snapshot; payloads outside declared inputs are never opened or hydrated.

Limits: 1..128 inputs; 1..128 explicit scope paths per category; native metadata
512 documents, 8 MiB and 30 seconds, plus the Repository limits; each in-memory
read at most 256 MiB. Read attempts consume the caller's remaining byte budget
before native I/O, including failed attempts. Repeated reads also consume budget.
There is no automatic alternate-store, latest-version or stream fallback.

Diagnostics contain stable bounded codes/kinds, not paths, prompts or returned
bytes. Missing, Integrity, Conflict, Stale, Denied, Busy, Unsupported, Budget,
Invalid and NeedsRecovery remain separate. Only Busy is a transient-contention
retry candidate for the same request within the remaining caller budgets.
Missing needs explicit byte recovery; Integrity needs investigation; Invalid,
Denied and Unsupported need corrected configuration or input; Conflict/Stale
require a fresh explicit source decision. NeedsRecovery does not claim absence
or safe replay. This read-only adapter has no publication outcome to reconcile.
Previews contain private IDs and hashes: they are local control data, not
privacy-safe public receipts. Store expanding control/evidence asset-side.

**This is not complete production adoption.** Shared byte guards end at process
exit. The claims and candidate adapters below add durable declared obligations
and retained publication. Owned preparation is covered by the component below. Complete original task
control retention and the generation-fenced handoff Lifecycle are still open. This
package does not acknowledge task completion or retire scratch.

## Durable declared job claims

`claims::Handle` composes the existing public guarded Namespace; it creates no
new storage engine, journal, lease timer or automatic enrollment.

- The caller freezes the complete declared flat resource Selection, finishes its
  metadata provider and independently opens a `MetadataReader`. Include inputs,
  request/packet/tool/runtime bindings and other required resources explicitly;
  the adapter does not infer task closure or asset availability.
- Pre-enroll an independent attempt as Kind::Job using the existing trusted owner
  enrollment procedure. `Handle::prepare` observes that empty source and fixes
  original Select operation IDs, expected revision, owner, root, context and
  namespace minimum. It writes nothing. A namespace policy decides placement.
- Retain `Handle::encode()` bytes asset-side before admission, pin their exact
  Object in the caller's original operation control, and keep that pin outside
  the submitted bytes. `Handle::decode(bytes, expected_object)` refuses changed
  bytes and wire data over 16 KiB. An owner key is not authentication: the caller
  still owns authorization. A hash supplied by the same untrusted payload is
  not an independent expectation.
- `admit(namespace, metadata_reader, limits)` adopts the exact desired claim.
  Keep managed byte guards alive across this call when bridging existing bytes
  into persistent protection. No bytes are opened, transferred or acknowledged
  by this operation. The job namespace must be in the actual pool's full pinned
  roster before participating native pruners can honor its obligations.
- `status` observes the original outcome plus current source state. Unapplied,
  Active, Historical and Advanced are observations, never execution guards or
  proof of custody. Exact replay recovers the old receipt without making it
  current. Reopen the original namespace at its minimum after uncertain errors;
  never allocate replacement operation IDs to bypass uncertainty.
- `cancel` uses the original adopted revision and an explicit cancel operation.
  Persist that operation before calling; retry with the same ID. Cancellation
  retains the old selection as a predecessor under fixed history placement.
  No predecessor retirement, deletion or scratch cleanup follows. Stale writers
  cannot cancel a newer source revision.

### Bridge held native inputs to the original claim

Use `GuardedInputs::admit_claim` after all named inputs have been opened and
verified through `InputReader::read`. Set the original Select's context to
`Object::from_bytes(&preview.encode()?).sha256`; its flat resource tree must
include every declared input Object. The root also binds any additional controls
explicitly declared by the caller. Retain the exact preview and original handle
asset-side; a context hash alone does not retain their bytes.

The bridge checks complete live input guards, exact preview context, full roster
membership, and Job current-placement policy requiring this physical pool. It
opens the namespace at the location already admitted by the held pool profile,
not an arbitrary caller namespace handle with the same copied identity. Read
and configuration guards remain held throughout coverage verification/adoption.
Dropping the reader afterward releases live guards while durable current claims
continue to suppress participating removal. Check the returned current state;
a historical receipt is not current protection or an execution lease.

Coverage reads only the frozen flat metadata tree, at most 512 entries, 4,096
reads and 4 MiB, also capped by caller Limits. Its reads, bytes and elapsed time
are subtracted before Namespace admission; it does not scan other catalogues or
open more payloads. Missing-input/preview mismatch, wrong roster/pool policy,
missing/corrupt metadata and budget refusal prevent this admission. Extra caller
resources are not silently materialized or given byte-custody acknowledgements.
Roster locations remain trusted owner enrollment. Replacing a namespace/epoch
or its writer/pruner boundary needs the separately qualified handoff; these
fixtures do not establish a safe migration for unknown legacy actors. This
bridge protects the declared inputs, not a complete task/attempt Lifecycle.

No clock expires these claims. Days-long handoffs require no renewal writes;
status checks can observe their continued protection. Process exit and worktree
closure do not withdraw them. Source limits remain 1,024 sources, 64 predecessors,
16,384 commands and 16 MiB journal bodies per namespace. Exhaustion refuses;
continuation/real workload qualification remains a P5 gate. Metadata closure
failures are NeedsRecovery (unknown), never a false claim of missing payload or
zero demand. Missing/corrupt bytes remain separate managed-read diagnoses.

Qualification uses synthetic temporary Git repositories without media,
providers, renders, cache enrollment or native removal. Run:

```sh
cargo fmt --manifest-path crates/reel-workflow-git-asset/Cargo.toml -- --check
cargo clippy --manifest-path crates/reel-workflow-git-asset/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path crates/reel-workflow-git-asset/Cargo.toml --all-targets --locked
```

Linux checks qualify metadata contracts only; native managed byte access still
has the pinned SDK's Windows/runtime/binding/roster obligations. Full P4 exit
requires the complete generation-fenced handoff, owned preparation and joined
crash/uncertainty acceptance, beyond the component fixtures below.

Native bridge qualification is opt-in on Windows with explicitly admitted
`GIT_ASSET_TEST_GIT_EXE`, `GIT_ASSET_TEST_ANNEX_EXE` and
`GIT_ASSET_TEST_ANNEX_PATH` (runtime search directories). These are test-only
inputs; ordinary construction never discovers or initializes caches.

```sh
cargo test --manifest-path crates/reel-workflow-git-asset/Cargo.toml --test native --locked -- --ignored --test-threads=1
```

The native fixtures create only owned temporary synthetic pools and metadata.
They qualify ordinary/bare annex, bare Git metadata, guard-to-claim continuity,
enforcing roster locations, exact historical selection, missing bytes, wrong
policy/context, incomplete closure and budgets. The normal CI workflow does not
install a native runtime and does not claim this opt-in qualification.

## Returned candidates and retained handoff

`candidates::Candidates::bind(ticket, original_json, packet_id)` validates the
original task/attempt and declared outputs. It keeps the original returned bytes
(including whitespace), not a reserialized approximation. It exposes expected
roles `output/<name>`, optional `worker-receipt`, and `return-packet`, plus compact
private provenance pinning the ticket, exact raw reply and supplied provenance.
These are declarations, not authenticated provider identity or creative approval.
Bodies remain opaque bytes; media interpretation belongs to REEL's adapters.

1. Use the existing SDK to freeze one native candidate Plan per expected role,
   with the exact Artifact identity and `Object::from_bytes(set.provenance())`.
   Admit distinct Retention sources/owners and the existing witness/capacity/
   staging resources explicitly. Finish/reopen metadata before publication.
2. `Candidates::prepare(plans)` seals the complete original plan cohort. Persist
   `Prepared::control()` asset-side with an independent immutable hash BEFORE any
   effect. It fixes all original operations, roots and baselines. Recovery uses
   `Candidates::restore(control, original_object)`; it cannot rewrite a reply or
   replace an operation in that record. Store resource profiles, recovery minima
   and provider metadata durably as well; a returned packet hash alone cannot
   reconstruct all admission state.
3. Call `Prepared::apply(role, publisher, resources, byte_source)`. Its plans are
   internal; retries cannot accept substitute plans. `Candidates::packet_source`
   serves only the original JSON body. Other bodies use existing guarded sources.
   The SDK verifies bytes, accounts staging/native effects and records immutable
   unselected retention. No Git selected pointer or creative source is changed.
4. `resume` uses the original SDK recovery protocol. `reopen` freshly verifies
   retained bytes without producer input or staging. Uncertain effects remain
   NeedsRecovery with code `uncertain`; acknowledged missing bytes keep code
   `missing_acknowledged`. Do not allocate new operations or reupload implicitly.
5. Keep each returned `Retained` guard. `Prepared::gather` requires exactly the
   complete cohort, rejecting omissions, duplicate roles and mixed controls.
   The resulting `RetainedBatch` holds all byte/protection guards while exposing
   the original return-packet Artifact as `acknowledgement()`. Serialized receipts
   alone cannot construct this type. Inspect/read named candidates through it.

Bounds: original JSON and serialized plan control each 1 MiB; at most 128 output
artifacts plus worker receipt and packet; compact provenance 4 KiB. Existing
SDK/profile body, transfer, metadata, command and stage limits still apply.
Registration can precede bytes; completion and accounting can have separate crash
boundaries. Persist the original controls, preserve scratch on uncertain outcomes,
and reopen retained bytes before semantic completion or cleanup. Any required
independent downstream-owner adoption remains explicit. This component performs
no task completion, scratch deletion, claim withdrawal or creative selection and
does not yet implement `handoff::Lifecycle`'s original actor-generation guard.

The native returned-candidate fixtures publish a synthetic output and its exact
raw reply in ordinary/bare pools, then reopen both with the old producer and
project locations offline. Completed apply/recovery must avoid reopening producer
input or appending another registration. These are local opt-in component checks;
normal hosted CI runs their bounded parser/control counterparts.

## Owned named input preparation

`workspace::Preparation::capture(snapshot, preview, bindings, budget)` checks the
exact original input set, portable destination mapping, logical bytes and worst
case file-data allowance before filesystem or source I/O. Bindings map every
logical ID once to an ASCII relative filename beneath `inputs/`. Traversal,
reserved names, case aliases and file/directory prefix collisions are refused.
Limits: 128 inputs, 512 named directories, 200-byte relative names, 64 GiB per
file and 1 TiB aggregate. The preparation control is at most 256 KiB.

`Workspace::create(existing_parent, new_name, preparation)` explicitly creates
only new caller-owned scratch. It reuses REEL's existing guarded directory
identity API; it does not create a cache or capacity service. Persist the returned
`Ticket::encode()` bytes and preparation control asset-side with independent
original Object pins BEFORE materializing. Tickets are private locators and
ownership controls, at most 16 KiB; copying owner.json into a replacement
directory does not reproduce its physical identity.

`Workspace::materialize(byte_source)` streams each missing named input with a
64 KiB buffer using the existing SDK ByteSource port. For native pools use
`annex::managed::Reader` with the admitted full roster. Copies are checked by
hash and length before a no-overwrite hard link publishes the destination, then
checked again under held read guards. Existing matching files are verified and
reused without opening the source. Differing bytes are preserved and refused.
Only declared paths and four deterministic partial slots per input are inspected;
no directory inventory or unrelated asset hydration occurs.

An interrupted prefix is preserved; retry uses another unused slot. A complete
verified partial can publish without producer input. Four incomplete slots or a
corrupt complete partial require explicit recovery. No error, timeout or Drop
sweeps partials, overwrites files, deletes scratch, releases claims or marks task
completion. One workspace gate excludes competing preparation while other owned
workspaces remain usable. Named directory and file guards block replacement and
mutation during use. Keep the Workspace alive while consumers use its paths.

Recover with `Preparation::restore(control, original_object)`,
`Ticket::decode(ticket_bytes, original_ticket_object)` and `Workspace::reopen`.
`verify` reopens the exact staged inputs without Git metadata or cache access.
Control pins must come from the original admitted operation, not self-hashed
submitted JSON. Namespace/ownership guards require Windows; other platforms refuse
filesystem operations before I/O, while pure planning remains portable.

`Budget` fixes `logical_bytes`, `peak_file_data` and cooperative `millis` (1..300000)
for one materialize/verify pass. The conservative file-data bound counts four
partial slots plus final copies and bounded control bytes; hard-link sharing can
reduce actual allocation. This is not a total filesystem quota or capacity
reservation: directory/FS overhead and other writers need separately admitted
headroom. Native source verification and command budgets also still apply.
Time checks cannot interrupt a blocked kernel I/O. Exceeding a budget preserves
all data and never expires persistent job claims.

Windows fixtures cover actual child exit during copying, exact-ticket retry,
foreign bytes, stale directory identity, junction refusal, scoped exclusion,
source corruption, exhausted slots and cooperative time refusal. The native
ordinary/bare input fixtures compose exact historical resolution, managed reads,
durable claims and owned streaming preparation/reopen. No renderer runs.


## Exact original source-generation use

`Handle::hold(namespace)` requires the original admitted Select receipt, owner,
source revision and full selected Version, then returns the actual SDK source-use
guard. Equal bytes/revision from another operation do not satisfy the original
handle. Keep the guard alive for active work; new source mutations return Busy.
It performs no admission or publication. Drop/process exit releases only live
use; durable claims remain, and cancellation stays explicit. Old namespaces
return Unsupported and are not upgraded. This API does not authenticate an owner,
bind a native pool's actual roster root, or verify/retain payload bytes. Complete
ProtectedJob/Lifecycle still requires those separate joined protections and
original task/control retention. Retained-result verification after settlement
must not revive an old active generation.


`GuardedInputs::protect_claim(handle, metadata, limits)` consumes the already
verified input reader, repeats actual roster/policy/coverage checks, and acquires
the exact original claim's source use at that configured root. ProtectedInputs
owns both source-generation and actual managed byte/config/namespace guards until
Drop; it still implements InputReader with the original byte budget. It accepts
no caller substitute namespace path and performs no claim admission or payload
acquisition. All named inputs must already be held. This is a protection bridge,
not complete task/request/control retention or retained-result Lifecycle.
