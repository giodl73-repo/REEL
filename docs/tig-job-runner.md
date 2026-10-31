# Tig-backed bounded jobs

## Prepared-result recovery

The Rust `tig_job_runner::adopt_prepared_result` API resumes the narrow durable
boundary where cached request/evidence/output bytes and result metadata are
complete, and the source journal has recorded prepare but not adopt. Supply the
existing journal path, exact job ID, original admitted `JobContext`, original
runner profile and explicit verification budgets. It acquires only that source's
fence, requires the exact original recorded operation, verifies the retained tree
and guarded active-cache bytes, and then completes the original adoption. Repeated
adoption is idempotent; another owner, operation, revision or later job refuses.
The original success/failure outcome is preserved. This is generic retention
recovery, never a native producer claim or creative approval.

The API runs no tools and creates no payloads or stores. It reports
`staging_retired:false` and `authorizes_workspace_removal:false`. A source with no
prepared result remains held. Recovery of unretained scratch, retirement of an
old workspace and child-owner handoff are separate steps; this API does not
declare the full recovery/cutover milestone complete.

```text
reel-tig-job adopt-prepared <private-recovery-profile.json>
```

The strict profile has schema `reel.tig-prepared-recovery-profile.experimental-1`:

| Fields | Admission |
| --- | --- |
| `job_id`, `original_input` | Original runner job ID and exact input-job Selection (`root` Object and `context` digest) |
| `source_id`, `owner`, `expected_revision`, `retain_operation` | Original input owner's identity, revision and retention operation; not the prepared revision or a new operation |
| `journal_root`, `journal_minimum` | Existing journal and trusted minimum checkpoint |
| `metadata_root`, `metadata_minimum` | Existing metadata provider and trusted minimum checkpoint |
| `cache`, `coordination_root` | Explicit existing active CAS roots and shared source-fence directory |
| `budget` | `max_objects` (2–1024), `max_verify_bytes` (positive), `verify_millis` (1–300000 per read) |

The CLI checks the exact input-job metadata identity and absence of an output
claim, then requires that pin to match the original journal-owned input. It does
not reconstruct a compiler plan, bind a media runtime or declare an old producer
current. The recorded prepare must match the operation exactly; verified byte
read guards stay live through adoption. No directory is discovered or initialized.
Profiles and stdout retain private pins and must stay outside project Git.

A successful command means metadata adoption completed or replayed. Inspect
`succeeded` for the original job outcome: adopting a failed job correctly returns
`succeeded:false`. Both outcomes leave scratch and approval unchanged. Output
record proposal adoption and child-owner handoff remain explicit separate steps.

### Published packet without a recorded prepare

```text
reel-tig-job retain-published <private-published-recovery-profile.json>
```

Use schema `reel.tig-published-recovery-profile.experimental-1`, the same fields
as prepared recovery, and an explicit `published_result` Selection. This is an
admitted complete result tree already written by the retention sink, including
the request, evidence and exact output pins. This command does not discover that
tree, reconstruct a packet from arbitrary files or infer a successful render.
New retention packets bind their retention operation in the immutable header.
Published recovery requires that exact binding. Older packets lacking it remain
eligible only for recorded-prepare recovery; it is not inferred retroactively.

When the source still owns the exact original input and has no pending selection,
the command verifies the complete tree, scoped header, request/evidence agreement,
declared bounds and protected cached bytes before recording the original prepare
and adopt operations. Any verification failure leaves the source and journal
unchanged. A crash after prepare can be retried with the same profile; another
pending or current result refuses. Repeating a completed operation is idempotent.
The original input remains a predecessor.

The result schema is `reel.tig-published-retention-recovery.experimental-1`.
`prepare_recorded_now` distinguishes a newly recorded prepare from replay. This
does not change `adopt-prepared`: that command still requires a recorded prepare
and rejects a published-result admission. Both commands preserve the original
success/failure outcome and grant no producer, approval or scratch-removal claim.
Incomplete packets and unpublished workspace recovery remain separate work.

### Verify and retire recovered job scratch

```text
reel-tig-job retire-workspace <private-retirement-profile.json>
reel-tig-job retire-workspace <private-retirement-profile.json> --apply
```

This separate command requires a completed current result and the original
recorded prepare/adopt operation. It cannot adopt a pending result as a side
effect. The default verifies and reports; `--apply` explicitly authorizes removal
of that one owned workspace. It changes no source-journal selection or event.

Use schema `reel.tig-workspace-retirement-profile.experimental-1`, the prepared
recovery fields, and `retirement: {"workspace": ABSOLUTE_PATH,
"max_workspace_bytes": POSITIVE_INTEGER}`. Omit `published_result`. The cache's
explicit `coordination_root` must match the original capacity/fence root. The
workspace name must equal `job_id` and its canonical parent must be the enrolled
cache's exact `staging/caminos` directory. No cache or workspace is discovered.

Verification holds the source fence and protected cached request/evidence/output
bytes, then also protects every original request input and cached tool. Installed
tools must remain verified outside the workspace. The request in `job.json` must
match the retained bytes. Any existing declared output must exactly match its
retained candidate; an unretained or changed output refuses removal. Missing
canonical originals cannot be excused by remaining hard links in scratch.

An exact job's reservation record or lease refuses retirement, even if stale;
this command never breaks or releases it. Other jobs' reservations do not block
the operation. Short capacity metadata checks do not span hashing or removal.
The directory is bounded to 10,000 entries and depth sixteen, rejects reparse
points, and is rechecked immediately before removal. No cache payload, shared
LFS object, worktree or unrelated directory is removed. Windows mutation/deletion
guards are currently required; other platforms refuse this operation.

The private report distinguishes `workspace_logical_bytes_verified` and
`workspace_logical_bytes_removed` from disk space: `physical_bytes_freed` is null,
because hard-link view sizes are not physical reclamation measurements.
`cache_objects_removed` is always zero. Repeating after removal verifies the
retained packet/originals and reports `already_absent:true`. This does not release
input predecessors, grant approval or finish child-owner handoff. Incomplete
and unpublished workspace salvage remains separate.
The report preserves the original `succeeded` value: safe retirement of a failed
job's retained candidates does not make that job successful.
Before the first applied removal, the command atomically publishes a checksummed
private transaction in the existing coordination directory. It binds the source,
journal, result selection, operation, manifest and canonical workspace, plus
Windows volume/file/creation identities and a bounded snapshot of the directory.
The transaction is local recovery metadata, never a producer/approval assertion
or a second authored asset inventory. Dry-run does not create or advance it.

After process interruption, retry rechecks the complete retained packet and all
original bytes. It accepts only an unchanged surviving subset of the recorded
entries in the same directory identity. Removed children may change directory
timestamps; changed files, new entries and replacement directories refuse. A
missing `job.json` is accepted only with that exact prepared transaction; a
manifest missing without prior intent still refuses. Every remaining declared
output is checked against its retained candidate again.

After removal, the transaction is atomically compacted to a small completed
record that retains scope and original directory identity. Completion-write
failure can be retried from the prepared record even when the workspace is
already absent. A completed record cannot authorize deletion of a new directory
at the old path. `retirement_transaction` in the report exposes recorded state
and whether a prepared transaction was resumed. Transaction records are bounded
to 8 MiB before compaction, remain outside Git and do not change journal events.

## Set up shared stores once

The managed Windows CLI can explicitly create the metadata provider and source
journal through the existing Tig implementations. It does not create byte caches
or run setup from any other command. All worktrees share the resulting bindings;
do not create another store/journal for each job.

```text
reel-tig-job setup <private-setup-profile.json> <new-private-store-bindings.json>
reel-tig-job check-stores <private-store-bindings.json>
```

A setup profile looks like this; all directories in the example already exist:

```json
{
  "schema": "reel.tig-job-store-setup.experimental-1",
  "state_parent": "X:/project-state",
  "capacity_cache": "X:/byte-cache",
  "cache": {
    "path_policy": "configured-local-roots",
    "cache_roots": ["X:/byte-cache"],
    "coordination_root": "X:/shared-control"
  },
  "metadata_limits": {
    "nodes": 131072,
    "bytes": 134217728,
    "database_bytes": 268435456,
    "min_free_bytes": 67108864
  },
  "journal_project": "my-project",
  "journal_epoch": "initial"
}
```

Use the same shared capacity policy/coordination root as the other project
workers. Existing capacity policy defaults apply unless explicitly configured.
State and the new bindings file must reside on the selected capacity volume.
Setup admits database/rollback plus bootstrap and allocation-rounding allowances
before creating a unique owned directory beneath `state_parent`. It creates and
independently reopens both empty providers, then publishes the private bindings
with no overwrite. A handled refusal cleans only its own new directory; failed
cleanup retains conservative accounting for explicit recovery. An abrupt crash
can leave an unselected directory/stale reservation; automatic orphan discovery,
deletion and complete recovery commands are outside this setup operation.

The small bindings file has schema `reel.tig-job-store-bindings.experimental-1`,
`metadata_root`, `metadata_minimum`, `journal_root`, and `journal_minimum`. Keep
it outside Git and back it up as private control data. Use the metadata fields for
capture and the journal fields in managed job profiles. Check-stores opens those
named existing providers with their trusted minimum checkpoints; it does not
create missing roots or infer complete job coverage/custody. Later appends are
allowed; rollback/foreign identity refusals remain the provider's responsibility.
Neither technical setup nor checking installs runtime bindings or adopts assets.
Setup records initial trusted minimums. Retain the advanced checkpoints returned
by later admitted jobs as private control evidence; an initial minimum cannot
detect rollback of changes the checker has never independently witnessed. A
successful check against old bindings does not prove latest protected state.

## Capture a committed scene for the managed CLI

With `tig-publication`, the explicit command below freezes a committed native
source and independently recompiles its frozen context before publishing a small
private worker-context fragment. It does not render or adopt a job.

```text
reel-tig-job capture-context <private-capture-profile.json> <new-private-fragment.json>
```

The capture profile has these fields:

| Field | Meaning |
| --- | --- |
| `schema` | `reel.tig-native-capture-profile.experimental-1` |
| `source` | Existing committed `reel.tig-input-profile.experimental-1` object |
| `request` | Native scene request, including exact language/control slot policy |
| `metadata_root`, `metadata_minimum` | Existing metadata provider and trusted checkpoint |
| `metadata_cache` | Configured active capacity root on the metadata/output volume |
| `preparation_cache` | Configured active root for owned compiler scratch |
| `cache` | Shared capacity configuration with an existing absolute coordination root |

The output path must be new, absolute, and have an existing parent on the metadata
volume. The command never replaces an existing file. It reserves conservative
database/rollback and two-pass compiler capacity before opening a metadata writer.
It checks writer bounds again, freezes the source, finishes and reopens the store,
then replays into separate owned scratch. Context selection, descriptor, archive,
requirements and all three compiled delivery controls must agree. Scratch cleanup
must succeed before the output fragment is published. A failed capture can leave
unselected immutable metadata in the existing provider; it grants no retention,
source selection or eviction authority. This is cooperative admission, not a
filesystem quota or protection against unrelated external disk writers.

The fragment contains `context_mode`, `metadata_root`, `metadata_minimum`,
`source` (the archived source), `context`, and `request`. Copy these fields into the
managed profile's `worker` object and supply the independently pinned runtime and
output policy described below. Keep the fragment/profile outside Git: they include
private source identifiers, paths and admission settings. Stdout is count-only;
refusal diagnostics omit private provider/compiler error chains. Capture uses
named controls/alignment metadata and never decodes primary pictures or audio.

Use the explicit setup command above once, configure existing byte caches, and
bind the runtime independently. Setup and capture remove custom Rust bootstrap
and source-freezing steps; they are not automatic runtime installation or full
project adoption. Existing contexts retain their original adapter/runtime
obligations when tools change.

The optional `tig-publication` feature exposes `reel::tig_job_runner::run`.
It connects an already adopted planned Tig job context to the existing
`reel-assets` runner. It does not create another runner, cache or inventory.

Provide the exact read-back `JobContext`, authoritative `SourceJournal`, owner,
source revision, operation key, existing metadata directory and trusted minimum
checkpoint. The explicit cache `Config` and shared coordination directory must
use the same enrolled roots as the other project workers. Names and drive letters
are project configuration; an episode cache on F and music cache on E are examples.

```text
planned job metadata -> finish/reopen/readback -> owner prepare/adopt
                                                  |
                                     one source-scoped job fence
                                                  |
job request -> exact role/size checks -> capacity reservation
                                                  |
                          pinned inputs + bounded owned workspace
                                                  |
                       existing Windows process-tree containment
                                                  |
                     verify outputs / salvage declared failure outputs
                                                  |
                       existing CAS publication + bounded evidence
                                                  |
                 result tree -> finish/reopen/readback -> owner adopt
                                                  |
                   fresh retained-byte checks -> retire owned scratch
```

Inputs must belong to the admitted Primary, Control or Interpretation roles;
tools must belong to Runtime. Exact hashes and sizes are required before source
lookup or capacity admission. The runner still verifies actual local bytes and
keeps read protection during execution. This trusted-command runner is not a
sandbox for hostile tools. Installed-tool pins do not discover neighboring DLLs
or the complete environment.

Retention links the original job context, exact request, compressed evidence and
each verified output. It finishes metadata, reopens and checks it, then uses the
same journal authority, owner, source revision and current selection for owned
prepare/adopt. The input root remains a predecessor obligation. Arbitrary command
results remain unproven: technical success does not create a native production
claim, creative approval or delivery status.

Execution failure can retain bounded declared candidates and diagnostics. A
retention failure preserves the workspace and does not turn a successful producer
into a failed producer or invoke a contradictory second retention attempt.
Ambiguous journal transitions keep their existing requirements; recovery needs
the exact owner state. `retain_workspace: true` remains an explicit diagnostics
exception: it skips this retention/retirement handoff and keeps scratch.

`reel_assets::jobs::run_admitted` exposes the reusable ports independently of Tig:
`RunContext` supplies `Config`, a role-sensitive `ResourceAuthority`, and a
`RetentionSink`. The sink receives immutable runner-created descriptors, not
deserialized success JSON. Returning success promises durable owned retention;
the runner then independently checks canonical bytes and exact workspace scope.
Legacy `run` and checkpoint-resume paths keep their registry adapters.

The current bounded runner executes on Windows. Existing scratch reservations
are conservative and stay in force throughout work; this does not integrate
Tig's complete physical-allocation ledger. Metadata availability and one admitted
authoritative journal remain caller obligations. Native managed CLI execution,
all-worktree claim coverage, interrupted handoff recovery commands, independent
archive custody and retained-cache eviction remain separate work. No source or
cache payload is removed by this integration.

## Native coordinator and worker

`cargo run --locked --features tig-publication --bin reel-tig-job -- run profile.json`
runs an explicitly configured frozen native scene through the bounded runner.
It requires Windows, existing metadata/journal/control/CAS directories and trusted
minimum checkpoints. It does not install a runtime or initialize those roots.

```text
private profile + frozen context + explicit tool bindings
                       |
       coordinator binds its actual executable and programs
                       |
          restore -> recipe -> validate bounded job request
                       |
        freeze/readback JobPlan -> owner prepare/adopt
                       |
        pinned same-binary child in bounded owned scratch
                       |
       child rebinds actual host + FFmpeg + FFprobe + dependencies
                       |
       restore exact context/recipe -> compile/render/check
                       |
       exact complete declared output set + receipt + worker record
                       |
          Tig retention handoff -> retire disposable scratch
```

The private profile schema is `reel.tig-native-job-profile.experimental-1`:

| Field | Meaning |
| --- | --- |
| `worker.metadata_root`, `worker.metadata_minimum` | Existing metadata provider and trusted minimum checkpoint |
| `worker.source` | Admitted `reel.tig-archive-input-profile.experimental-1`; Git is not needed during execution |
| `worker.context` | Exact frozen native context Selection, not a latest lookup |
| `worker.context_mode` | `declared` (default) or explicitly `compiled`; compiled mode replays the archived language-scoped native interpretation |
| `worker.request` | Native scene Request matching the frozen descriptor |
| `worker.runtime` | Private Bindings: actual CLI host byte identity, explicit FFmpeg/FFprobe FileRefs, named dependency FileRefs |
| `worker.outputs` | Explicit complete native output filenames plus `receipt.json`, each mapped to a positive maximum; omit when using `output_budget` |
| `worker.output_budget` | Alternative `{ "default_max_bytes": ..., "overrides": { ... } }` policy; exact override names must exist in the derived contract |
| `worker.record_outputs` | Optional output-to-record proposals: exact archived template Reference, native output name and selected/stream selector; see below |
| `worker.git_handoff` | Optional retained `tig-propose` profile with explicit Git destination/identity and one canonical path per candidate asset |
| `cache` | Existing reel-assets Config, including an existing absolute shared `coordination_root` |
| `preparation_cache` | Compiled mode only: an existing absolute active root listed in `cache.cache_roots` for owned coordinator preparation |
| `journal_root`, `journal_minimum` | Existing authoritative owner journal and trusted minimum checkpoint |
| `source_id`, `owner`, `expected_revision` | Exact caller-owned job identity and current revision; zero is required for a new source |
| `start_operation`, `retain_operation` | Explicit distinct-purpose stable operation identities |
| `budget_bytes`, `scratch_bytes`, `timeout_seconds` | Complete workspace/publication allowance, scratch allowance and bounded step deadline |

The profile is limited to 128 KiB. The inline worker admission must fit 64 KiB;
there are at most 511 native outputs. Native outputs are retained under
`native/<filename>` roles, with a separate bounded `worker.json` completion record.
All native receipt outputs must be declared; extra or omitted names cannot report
successful retention. Budgets must include two copies of declared output maxima
for atomic publication, control bytes and scratch. Installed runtimes execute in
place and primary inputs resolve directly from the existing cache enrollment.
The worker's compiled controls and intermediates occupy its bounded scratch.

### Compiled context preparation

Set `worker.context_mode` to `compiled` only for a frozen compiled render context.
The coordinator and worker use the same projected recipe and output contract;
neither infers this mode from a report. Omitted mode retains the declared-context
workflow. A declared-context profile must omit `preparation_cache`.

Before choosing budgets, run:

```text
reel-tig-job preparation-budget profile.json
```

This reads the bounded profile and filesystem allocation geometry. It reports
coordinator preparation bytes and the minimum worker scratch allowance without
opening source metadata, admitting a job, creating files or scanning caches.
The worker allowance covers three compilation passes at the largest allocation
unit among the configured active roots. It is a preparation minimum; provide
additional scratch for rendering and include output publication in the complete
workspace budget. The CLI refuses insufficient declared scratch before owner
transitions and does not increase a budget automatically.

Compiled preparation shares a 64 MiB logical write allowance and 4,128-file limit
per pass, with a 4 MiB maximum per document. The coordinator reserves two passes
through the existing shared capacity ledger before creating its owned
`native-prepare-<reservation-id>-*` directory. It removes that directory before
releasing capacity. Failed cleanup retains the capacity record for explicit
recovery and reports its reservation ID. A process interruption also leaves
conservative accounting; automatic crash cleanup is not implemented here.
Worker preparation occurs inside the already admitted job workspace.

The estimate includes data allocation rounding and a per-entry allowance.
Configured free-space policy must cover filesystem administrative overhead and
external writers. These logical bounds and reservations are not a physical
filesystem quota. Preparation does not create another asset cache, select new
masters, switch installed runtimes or authorize payload eviction.

The coordinator keeps its verified runtime guards while the child runs. Both
processes derive the producer from the same declared runtime manifest and exact
native recipe. The coordinator pins the private ticket hash/size in process
arguments; a changed ticket refuses before parsing or tool/metadata lookup.
The child receives no journal handle or write authority for owner
transitions. A hidden `worker` subcommand is the trusted internal child entry;
invoking it directly does not provide admission, containment or retention.

Success reports the retained outputs and resulting owner-journal checkpoint.
It does not update project `.tig` files, approve a master or publish externally.
Optional record proposals can export a scoped unsigned native claim, as described
below. The generic retained result deliberately
reports `production_claim: null` and incomplete claim coverage. Metadata validity,
actual renderer checks, owned byte retention and creative selection stay distinct.
Errors do not reset source state: interrupted starts/handoffs require exact owner
recovery, and retention uncertainty preserves scratch. These profiles and receipts
contain private local information and are not shareable public diagnostics.

## Deriving outputs before rendering

Supply `worker.output_budget` instead of `worker.outputs` to derive the complete
native output list. For example:

```json
"output_budget": {
  "default_max_bytes": 268435456,
  "overrides": { "receipt.json": 65536 }
}
```

`reel-tig-job plan-outputs profile.json` runs a **retained bounded planning job**.
It verifies inputs and may invoke the explicit media probe; it does not encode a
scene. It publishes only `planning.json` plus the runner's request/diagnostics and
adopts the result root. This is an owned job with capacity/time/scratch admission
and durable metadata transitions, rather than a read-only or dry-run command.

`reel-tig-job run profile.json` with that policy first runs planning, verifies the
exact retained report bytes and current owner/revision/root, then derives the
render request. The actual worker rechecks the full native output contract before
encoding. Existing explicit output-map profiles still work and now reject an
incomplete output set before spending the render.

```text
new source@0
    -> planned native inputs@2
    -> retained planning result@4      (input predecessor retained)
    -> same planned native inputs@6   (planning predecessor retained)
    -> retained render result@8        (all three predecessors retained)
```

These numbers are source-journal revisions, not Git commits or asset versions.
Each phase uses its own scoped operation keys and the existing per-source fence;
foreign, stale, pending or aborted result state refuses. A planning report is read
only after the owner check and must match its CAS pin, exact retained tree/header,
source context, recipe, producer and budget policy. Both phases keep their earlier
requirements; dropping them or reclaiming their bytes needs separate lifecycle
authority. Use fresh attempt operation identities for a new request, not a reset.

The renderer and checker share `scene_delivery::output_contract` for base video/
audio, overlays, fonts, post-compose camera files and Motioncraft evidence. Its
`raw_pcm24_stereo_bytes` and `raw_rgb24_video_bytes` describe one uncompressed
stream's data, excluding container/encoding overhead. They are reference sizes,
not predicted compressed sizes or physical allocation. The report's
`atomic_native_output_bytes` is twice the sum of declared native output maxima;
the final job also needs scratch, controls, the worker completion record and any
optional record-proposal artifacts. Proposal maxima are added separately during
render-request admission; they are not included in that planning-report field.
Overflow and unknown/zero overrides refuse. Planning reserves the configured
scratch allowance capped at 64 MiB for metadata-only work, plus its bounded ticket,
a 256 KiB request-control allowance and
two 64 KiB report copies. It must fit the profile's workspace ceiling, but does not
reserve the full render allowance. The render phase keeps the configured full
budget and scratch allowance. Exceeding the planning control cap fails the bounded
job; it does not borrow the render allowance. These reservations still require the
cache's normal free-space margins.

Native requests explicitly set `output_count_limit: 512` in the reusable runner;
other requests keep the default64 unless they explicitly select a bound from1
through512. Intake, per-file/path, diagnostics/evidence, tree and total workspace
bounds remain in force. Large evidence sets are refused explicitly when they
exceed these bounds; they are never silently truncated into a successful result.

## Proposing native asset records

An optional `worker.record_outputs` mapping exports proposed `.tig` bytes from
an existing planned asset record in the same frozen source archive. For example:

```json
"record_outputs": [
  {
    "output": "master.mkv",
    "template": {
      "id": "delivery/example",
      "object": { "sha256": "<exact archived record SHA-256>", "bytes": 1234 }
    },
    "selector": { "kind": "selected" }
  },
  {
    "output": "master.mkv",
    "template": {
      "id": "delivery/example",
      "object": { "sha256": "<same archived record SHA-256>", "bytes": 1234 }
    },
    "selector": { "kind": "stream", "slot": "es" },
    "stream_class": { "role": "compiled-output", "group": "language" }
  }
]
```

The hash placeholders above must be replaced with actual archive pins. Definitions
must already be admitted by `worker.source`. Output records cannot be production
input records for this job. Stale/foreign templates, duplicate asset selectors,
conflicting template pins, unknown outputs and incompatible stream classes refuse.
A stream selector requires explicit role/group; a selected selector forbids them.
One native output can populate several distinct selectors. The mapping has at most
128 entries and must fit the existing worker/profile and total-output bounds.

After rendering and independent native checks, the sealed candidate produces an
unsigned claim and these retained auxiliary files:

```text
production-claim.json
record-candidates.json
records/<SHA-256 of asset ID UTF-8>.tig   (one per unique asset)
```

Each auxiliary artifact has a 64 KiB maximum declared before execution. Aggregate
atomic publication, scratch and evidence budgets still apply. These files travel
through the existing CAS/owned-result handoff before scratch retirement; they are
not written into project Git. Planning alone does not generate them.

The proposed record preserves authored fields and unmapped payload slots. It
clears status and old metadata assertions, retains the previous template Reference,
and records exact claim/receipt pins plus the mapped selectors in
`metadata.production`. Changed stream provenance pins the new claim and receipt.
Unmapped history remains untouched. The claim identifies exact asset, definition,
selector and produced bytes; it does not depend on the candidate record's own hash.

Before reporting `record_candidates`, the coordinator independently checks its
current completed owner/revision, retained tree/header, exact artifact bytes,
original recipe/context, template/definition binding and every claimed subject.
The returned paths are private retained-cache locators, not canonical record paths.
`current_selection: false` and `complete_semantic_coverage: false` remain explicit.
Binding validates the current native envelope and authored-field contract; it
does not execute arbitrary JSON Schema or all project semantic rules. The generic
job still reports `production_claim: null` and incomplete global claim coverage.
These proposals provide no human approval, release, complete custody, cache
eviction authority or automatic canonical worktree application.

## Retaining the Git handoff request

With `record_outputs`, optionally supply:

```json
"git_handoff": {
  "git_executable": "C:/Program Files/Git/cmd/git.exe",
  "repository": "C:/src/example",
  "base": "<exact base Git commit ID>",
  "identity": {
    "name": "Example Creator",
    "email": "creator@example.invalid",
    "unix_seconds": 1700000000
  },
  "message": "Update native delivery candidate",
  "paths": { "delivery/example": "deliveries/example.tig" },
  "protection": "prevent-mutation"
}
```

These are explicit destination settings, not a new asset inventory. `paths` must
map every unique candidate asset exactly once, without duplicate/escaping paths.
The base must be a real exact Git commit when the later Git command runs. Git
identity/message, mappings and profile bounds are checked before media encoding.
No Git program, repository or working file is accessed while exporting the profile.

The worker ticket freezes the job's existing output CAS roots. Those roots derive
the profile's byte-source list; there is no separately authored cache list. The
job declares one extra64KiB artifact, `git-proposal.json`, in its existing output
budget. It pins exact old template bytes and actual newly retained record objects.
The candidate index also pins the profile. Parent readback checks those bytes
against frozen destination settings, roots and record pins before returning
`record_candidates.git_proposal` with its object and private retained path.

After rendering, restore Git access and run the generic authoring tool separately:

```text
tig-propose check <retained-profile-path>
tig-propose create <retained-profile-path>
git show <returned-proposal-reference>
git cherry-pick <returned-proposal-commit>
```

Profile creation can run with Git offline. Check/create retries consume retained
small records rather than rerendering. They do not update the native job's source
ownership or authorize retirement of its retained roots. Git handles ordinary
merge conflicts; a conflict-free application still needs native schema, current
recipe/freshness and delivery/approval gates. The profile does not claim its base
commit represents every current production input. Other fixes can make a valid
retained render stale without invalidating its exact historical bytes/claim.

Keep these profiles and their paths/hashes private and immutable in the cache.
An automated caller resolves the returned profile Object through its trusted
byte-source enrollment and holds its verified read guard through CLI execution;
on Windows, `VerifiedRead::with_path` exposes the protected path. A filename alone
does not establish that a cached executable request is still the checked object.
Correcting destination settings creates a separate request; never edit retained
objects in place. Preserve old runtime/context obligations when adopting a new
vendored Tig/REEL source descriptor; intake is not an automatic runtime switch.

## Compare retained native output without rendering again

```text
reel-tig-job compare-retained <private-comparison-profile.json> <new-private-report.json>
```

This explicit read/CPU operation replays a frozen compiled scene, checks the
retained native output set, and compares its master with an independently pinned
reference. It does not render, select a candidate, publish, or evict assets.

The strict profile has schema `reel.tig-retained-comparison.experimental-1`:

| Field | Source and meaning |
| --- | --- |
| `metadata_root`, `metadata_minimum` | Existing provider and trusted checkpoint |
| `source`, `context`, `request` | Original captured worker fragment |
| `producer` | Independently admitted original producer Object, for example the earlier planning result; do not infer admission from the claim being checked |
| `claim`, `receipt` | Exact retained candidate claim and native receipt Objects |
| `outputs` | Map of native relative output names to exact Objects; remove the `native/` retention prefix and exclude `receipt.json` |
| `reference` | Independently selected baseline Object; comparison evidence, not a production dependency |
| `byte_source` | Existing enrolled CAS places, with `required: "prevent-mutation"` |
| `runtime` | Current validator executable, FFmpeg, FFprobe, and dependency bindings |
| `cache`, `preparation_cache` | Shared capacity policy and existing owned-scratch volume |
| `max_output_bytes` | Maximum sum of native output sizes plus receipt size |

An Object contains `sha256` and `bytes`. Validator host bindings must identify the
executable actually running this check. Keep the independently admitted original
producer distinct: a newer validator does not retroactively become the producer.

The command reserves bounded compiler and copy scratch, reads protected cache
objects, and copies only the named native outputs into its owned check directory.
It never makes writable aliases to cache objects. Receipt names, pins, native
plan, output contract, claim context/producer and subject roles must agree before
the independent native checker runs. Every comparison probe and decode uses the
declared validator tools, including when PATH is empty. Comparison checks exact
native clocks, frame count, picture planes, displayed RGB, color metadata and PCM.

The private report must be a new absolute file with an existing parent on the
preparation volume. Owned scratch is cleaned before bounded report publication.
Stdout contains counts and booleans only. A valid content mismatch writes its
report and returns failure; invalid admissions or corrupt bytes fail without a
comparison report. Existing reports are never overwritten. No metadata writer,
source-journal adoption or Git operation is involved. Reports retain exact pins
and must stay outside Git. Equality does not transfer human approval or prove
complete semantic coverage, execution attestation, custody or eviction safety.
