# Public git-asset input adapter (experimental)

An isolated Rust integration for REEL's `publication::InputReader`. It pins
public git-asset at `3ddeab5572f9479475134e6b7334a26effcb9277`, with its own
Cargo.lock. It preserves the existing private vendor and native workflow route.
No existing job is upgraded or rebound by adding this package.

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
exit. The claims adapter below adds durable declared obligations, but native
pool/roster integration, owned workspace preparation, returned-result ingestion,
retained publication and the complete handoff Lifecycle are still open. This
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
requires separate native persistent-job and retained-publication fixtures.
