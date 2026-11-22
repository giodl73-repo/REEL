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

**This is not durable job protection or production adoption.** Shared guards
end at process exit. This package cannot yet hold manual handoffs for days,
materialize an owned workspace, ingest or retain returned results, acknowledge
task completion, retire scratch, select candidates or authorize eviction.
Persistent Job-class admission and retained reopen-before-completion are the
next P4 slice, using existing public primitives. No timeout deletes bytes.

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
