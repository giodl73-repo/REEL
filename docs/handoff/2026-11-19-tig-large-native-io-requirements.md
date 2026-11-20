# Tig requirements from the large native result qualification

REEL now has concrete negative end-to-end evidence, rather than only a scale
estimate. The [original-operation fixture](2026-11-19-large-native-return.md)
reused an existing pool, declared explicit 4 GiB result / 8 GiB projection bounds
and attempted to retain an existing 3,600,000,001-byte synthetic file. Input
protection, old-slot close/reuse, immutable candidate work and output claims
succeeded. Initial ingress returned `NeedsRecovery`; one original-identity resume
advanced publication but failed retained-output preflight. Exact native location
is now reported. Retained byte proof/acknowledgement and task completion are not
established. Prior movie history still verifies.

## Needed interfaces and qualification

1. Caller-bounded source verification, native staging/copy and reopen verification
   deadlines, separately from native command timeouts and producer execution.
   REEL currently declares 300,000 ms for local per-file verification; five-hour
   render windows are a different budget. Preserve legacy defaults and exact
   profile/operation pins through explicit versioning. Fixed-size buffers and
   checks between OS reads/writes must remain; describe blocking-I/O limitations.
2. Structured phase/error results distinguishing source read/hash, free-space
   admission, staging/copy, reinjection, native reopen, integrity and retained
   handoff. Keep Missing/Denied/Corrupt/Busy/Budget/Unsupported/Conflict/Stale and
   uncertain outcomes distinct. The existing generic I/O errors do not establish
   the failure cause. REEL will not parse stderr or infer absence from a timeout.
3. Exact reconciliation of original publication/staging handles, expected native
   key, source identity and operation/candidate WorkRef after uncertain replies.
   Expose retained partial-attempt references where needed; bound retry/scratch
   growth. Preserve native/source bytes and positive claims. Do not allocate a
   fresh logical operation or delete unknown staging to make a retry succeed.
4. A pinned qualified source/tool packet for Windows/NTFS exercising this byte
   scale, original-operation resume, native reopen, retained-owner acknowledgement,
   source close/reuse history and missing/corrupt/timeout refusals. A byte allowance
   of 64 GiB alone does not qualify the elapsed-time or recovery path. Qualify
   constrained/contended timing as well as a favorable cached read.
5. A defined adoption/recovery boundary for already-pending work. Preserve its
   original source, task/attempt, activation, WorkRef and runtime evidence; do not
   silently retarget an existing pinned REEL handoff to a newly built executable.
   The current pending fixture must stay protected until an explicit qualified
   recovery/retained-owner handoff is available.

The reusable storage foundation remains the right boundary. This is not a request
for another cache engine, lease expiry, automatic deletion, latest-byte fallback,
selection, or bypass of 16 actors / 128 changed paths. The first required scope
is the existing explicit single store; it does not require pretending that the
newer multi-pool reader is already integrated into REEL.

## Source status

REEL's unchanged vendored qualified SDK has fixed 30-second loops in
`crates/tig-git/src/annex/workflow/publication.rs::transfer` and
`crates/tig-git/src/annex/workflow.rs::hold_copy_with_guards`. The observed I/O
failures do not by themselves prove that those loops caused either failure.
They show that byte allowances alone and the current diagnostics are insufficient
to qualify the workflow.

A read-only check of current `C:/src/Tig` also finds fixed 30-second loops in
those files. The current workflow, publication and reader files **do not match**
the older EVID-162 `qualified-source.json` entries. Do not treat the live checkout
as that qualified package, overwrite REEL's vendor pins, or use ancestry commit
`c62c58c` as a substitute for a manifest. Supply a new exact packet for any fix
or identify an already qualified interface with the required bounds.

## Preserved evidence / resume identity

Root: `F:/src/tig-dev-temp/reel-large-native-return-20261119-v1/job`.
Task: `841221500ab7dcc5f9f2022a6e61250d4a5846e21065110c5cae3a5954ecc7a1`;
attempt `large-attempt-1`; operation `large-native-result-original`.
The immutable `candidate.json` records the original registration and WorkRef;
`profile.json`, `receipt.json`, `result.json`, `returns.json` and the journal
preserve the exact native admission and return body. Inspections show complete
candidate work, positive `slot` claims and unchanged Index checkpoint sequence
89. The journal remains `ImportPending`; no retained acknowledgement exists.

The original effectful release CLI is pinned to SHA256
`07fecbd917c9c423e1a693a173445c3762ea9242a4a94f1264e1fbd300a5f78c`.
The status certificate and full immutable logs are linked from the fixture report.
Keep this job, original source reservation and native copies/staging intact.
No real S4/S5 job, media, recording, approval, project profile or selection changed.
