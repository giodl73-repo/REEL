# M12b.2: source and native session boundary review

Status: implementation in progress in an isolated lane based on merged M12b.1
`4dc0522` / [#403](https://github.com/giodl73-repo/REEL/pull/403).
One assistant applied the repository role checklists. No independent review,
actual endpoint operation, playback, capture or human approval is claimed.

The [runtime design/test map](../installation-native-runtime-v1.md) preserves
the full [M11 contract](../offline-installation-v1.md). The original file loader
is a component of that work; it cannot close M12b.2 by itself.

## Source checkpoint

| Lens | Finding / disposition |
| --- | --- |
| Rights/Provenance | Independently bind original compiler/producer bytes; never infer them from the receipt or current player. A real different CLI process now passes original checks, while a byte-correct replacement producer fails the original relationship. An original runtime need not be executable on Windows and is never run. |
| Sound | Fresh seal verification remains mandatory. Prepared and Transport share one frozen sample allocation after verification, retaining exact PCM24-to-f32 conversion, source sequence and quiet seams. Existing conversion/sample and original-input tests passed. No stream, gain, resampling or repair was added. |
| Editor | No cue/edition/order changes. Missing or altered original data refuses; pending native startup still needs actual-box retention and late-command tests before playback can be exposed. |
| Platform/Audience | Separate shape-only `validate-session` and explicit guarded-source `check-session`; neither is Run. Bounds, unsupported guarding and cancellation refuse visibly, with counts-only stdout. Actual terminal text persistence, standard line input and responsive controls remain pending. |

Story, animation, score/reconstruction and vocal adaptation are unused: no
authored media or wording is altered. Source rights, voice consent and release
stay upstream and cannot be granted by these checks.

## Execution

Exclusive target: `F:/tmp/reel-builds/media-m12-operator-isolated`, separate from
every other worktree. Windows 11 / Rust 1.98.0, debug symbols disabled for
dev/test, assertions retained, `-j2`.

- All-target/all-feature type checking passed (cold 4m00s).
- `cargo test --locked --all-features --lib installation -j2 -- --test-threads=1`:
  Final source/ownership run: 26 passed in 3.00s, zero failed. Two ordinary ignored fixture entries were
  launched explicitly by their parent tests; not counted as extra passes.
- `cargo test --locked --all-features --test installation_native_cli -j2`:
  Final run: five passed in 0.71s.
- `cargo test --locked --all-features --test spoken_programme_media installation:: -j2 -- --ignored --test-threads=1`:
  Final run: seven passed in 18.18s, zero failed/ignored. Includes actual separate-binary
  original checks and the six existing installation regression cases.

That is **38 distinct focused passing cases**. Final all-target/all-feature
Clippy passed with warnings denied (1m22s); formatting, whitespace, actionlint
and relative documentation links passed. An intermediate unused close-effect
assignment was removed before final qualification. This is not yet an overall
M12b.2 qualification record.

## Ownership findings resolved in this checkpoint

- Completed returns the actual box; new commands cannot disappear into an
  abandoned callback queue. A typed returned-owner result preserves caller
  control without manufacturing a backend fault.
- An atomic writer/return handshake defers ownership transfer without blocking
  the data callback. A controlled preemption test returns from fill before
  releasing the writer; 64 actual thread races verify accepted commands remain
  in the real core queue, or are refused and applied by its returned owner.
- Teardown before first fill retains the identical box/address and pending
  Stop, then yields a valid same-session restart ticket after explicit Restart.
- Report overflow faults **before** source progress. A 256-entry full queue
  leaves its actual source cursor at 256 one-frame submissions; the next silent
  fault buffer is not counted as another source frame.
- Reports preserve actual command refusals before subsequent Opened/media
  events sharing a sequence. Controls refused after Quit do not invent a new
  device/backend failure. Original providers explicitly deny external programs.

The isolated allocator fixture still proves no allocation/deallocation in fill,
including actual queue overflow and ownership return. Callback destruction is
not fill: it belongs to the native teardown path, whose final process/driver
cleanup is still required below.

## Output and source worker checkpoint

The same assistant applied the same four checklists to the new ports:

| Lens | Finding / disposition |
| --- | --- |
| Sound | Count actual source submissions separately from quiet intervals, paused callbacks, fault silence and completion padding. The full data-fill wrapper compares every float through variable buffers and source/interval pauses. An isolated allocator child covers wrapper clock/atomic work, handoff and report overflow; library/driver error handling is outside that claim. |
| Editor | Stream opening and `play()` completion do not authorize moving the core. Require a valid silent native callback first; cancelled/late opens retain the controller's identical box. A queued Stop before the first owned callback submits zero source frames. |
| Platform/Audience | One serial native backend, fixed explicit endpoint and no fallback. Per-callback bounds supplement the backend's period query. Close completion follows actual stream destruction. Correct the 120 s boundary: only Close may use the two-second cleanup tail. Actual foreground responsiveness, terminal behavior and contained cleanup remain open. |
| Rights/Provenance | Source guards and Prepared remain on one source thread; original lineage stays independent of the player. Initial prepare/fresh verification share one absolute deadline. Recheck permits remain opaque in-process objects. Cancellation/expiry is checked again at event reception so a queued success cannot revive a cancelled operation. Real-original integration checks retain the core's separate stale-permit gate. |

These ports do not expose a Run command or qualify a device. Constructing the
output port opens no endpoint; constructing the source port explicitly checks
originals only. Both destructors join actual thread teardown and can block on
an OS/driver operation. They must run inside the complete owned worker process;
their cooperative timers cannot substitute for containment and actual reaping.

Focused execution for this checkpoint:

- Installation unit filter: **41 passed**, zero failed, in 2.99 s. Three ordinary
  ignored fixtures were launched by their parent cases, including the full
  application data-fill allocator child.
- Explicit original-input integration filter: **eight passed**, zero failed or
  ignored, in 26.73 s. The real source-worker case retains metadata/producer
  guards through Ready and two rechecks, returns an applicable opaque permit,
  refuses a valid late permit after Stop, and releases guards on actual teardown.
- Native CLI: **five passed**, zero failed, in 0.10 s.

That is **54 distinct focused passing cases**, including the earlier source
cases; reruns/child entries are not counted again. Intermediate compile/lint
findings (private test-clock access, optional-value style and test tuple type
complexity) were corrected. No actual endpoint, playback or capture was used.
Final all-target/all-feature Clippy passed with warnings denied in 1m29s.
Formatting, whitespace, actionlint and changed-document relative links passed.

## Held worker identity and resource observations

The same assistant used Platform/Audience and Rights/Provenance checklists for
the narrow process-runner extension. The existing native launcher and shutdown
barriers are reused. Its optional observer obtains the worker PID and peak
working set from the held primary-process handle. It distinguishes observed
primary exit from completed job/descendant reaping, and errors cancel through
the same existing cleanup. Existing quiet-runner callers use no observer and
make no additional process-memory queries.

Controlled native-child tests check actual PID/handle agreement, a retained
32 MiB allocation measured before release, cancellation with the held process
already signaled on return, refusal before process creation, failed handle
observation, and a live descendant becoming signaled before Reaped is delivered.
An intermediate test assumption used `-1` as invalid; Windows accepts that as
the current-process pseudo-handle. The rejection test now uses null.

The metrics stay in-process: no private PID/path/resource receipt or portable
identity was introduced. A zero or unavailable measurement cannot establish the
profile's memory gate. Primary peak working set, tree committed-memory limit and
combined foreground/worker envelope remain distinct. The observer runs off the
controller at the existing runner cadence; it does not establish command
acknowledgement, audio quiescence or complete cleanup deadlines. Sound/Editor
have no new media/timing changes in this extension; their existing runtime
obligations above still apply. Other craft lenses remain outside this scope.

Runner execution: `cargo test --locked -p reel-assets -j2` passed **342 cases**,
zero failed. Its three ordinary ignored native fixture entries were invoked
by their parent tests; they are not extra passes. The library portion passed
240 cases in 18.11 s; binaries and integration targets passed another 102.
This includes all four new observation cases and the existing timeout, memory,
descendant, custody and CLI regressions. Package all-target Clippy passed with
warnings denied in 18.49 s. Root all-target/all-feature checking passed in
1m52s after the Windows ProcessStatus feature addition.
The 41 installation unit regressions also passed in 3.00 s against the changed
runner; final root all-target/all-feature Clippy passed with warnings denied in
1m56s. The allocation fixture waits for an explicit allocation marker as well
as the OS peak measurement, so process startup memory cannot stand in for the
known retained allocation.
After making that marker check tolerate an incomplete in-progress file write,
all four observation cases passed again in 1.39 s; final package all-target
Clippy passed in 3.67 s. Repeated cases are not additional passes. Formatting,
whitespace, actionlint and changed-document relative links passed.

## Private framing and actual local pipe checkpoint

The same assistant applied Rights/Provenance and Platform/Audience checklists
to `native::wire` and `native::pipe`. The [illustrated channel and byte layout](../installation-native-runtime-v1.md#private-local-channel)
separate the implemented transport from the pending controller integration.

| Lens | Finding / disposition |
| --- | --- |
| Rights/Provenance | Pipe names derive from a cryptographic per-run context; a protected current-user-only DACL and remote-client rejection constrain the actual local endpoints. The test queries the real pipe's OS descriptor, not just a constructed ACL. Both actual peers must match the held runner PID; client-side server verification is separate. No source, restart, voice or publication authority follows from framing, an invitation or a wire observation. |
| Platform/Audience | Separate one-way file objects permit bidirectional progress with blocked readers. Commands/reports use bounded 8/256 queues and frames of at most 8192 bytes including the header. Header/version/direction/nonce/sequence/bounds are checked before reading a body. Errors poison the receiver and overflow faults the channel; no resynchronization or reconnect. |
| Platform/Audience — cancellation | Repeated synchronous-I/O cancellation handles entry races; actual thread joins precede resource release and cleanup success. Tests cover unconnected waits, blocked reads, unread pipe backlog, EOF, wrong peers and cancelled readiness. Those port measurements do not establish the full 120 s/2 s foreground lifecycle or command acknowledgement gates. |
| Sound/Editor | No PCM, source order, intervals, cue text or native output configuration changed. Existing source/output tests reran alongside these cases. Generation bytes remain metadata: message applicability and actual core/permit checks are still required. |

The actual contained child exchanges exact Spanish text over both pipes after
its peer PID matches the existing runner's held process. It uses the retained
test image and actual job cleanup; its ordinary ignored child entry is launched
explicitly by the parent case. This is not a production fake-success mode.
Another native test holds a raw peer open after sending a malformed header and
no body; the real reader faults from that header without waiting for EOF/body.

Unit execution: **53 passed**, zero failed, in 3.72 s. Four ordinary ignored
fixture entries were invoked by their parent cases and are not additional
passes. This includes four portable framing cases and eight native pipe cases
added to the previous 41 installation cases. All-target/all-feature checking
passed before the tests (cold Windows feature build, 2m46s). Intermediate test
compile findings (Win32 control-word pointer type and scoped-child capture)
were corrected before the passing run. No actual audio endpoint was queried.
Final all-target/all-feature Clippy passed with warnings denied in 51.21 s.
All five native CLI cases passed in 0.83 s; all eight explicit original-input
cases passed with zero ignored in 27.45 s against the expanded Windows build.
That is **66 distinct focused passing cases**; prior runs and child fixtures
are not counted again. Formatting, whitespace, actionlint and changed-document
relative links passed.

At that pipe-only checkpoint, strict message bodies, request/runtime-hash authentication, command IDs and
generation applicability, original-text display acknowledgement, whole-session
deadlines and the operator console remain open. The current pipe transports
bounded bytes; it cannot be presented as a complete authenticated player.
Other craft lenses remain outside this unchanged-media scope; no independent
review, actual-device listening/usability or human authority is claimed.

## Guarded admission and command application checkpoint

The same assistant applied Rights/Provenance, Platform/Audience, Sound and Editor
checklists to the [runtime/message admission](../installation-native-runtime-v1.md#guarded-runtime-and-message-admission)
components and generation-stamped command path.

| Lens | Finding / disposition |
| --- | --- |
| Rights/Provenance | Bind the actual current executable and exact raw request independently from the original producer. Retained lock bytes must match this binary's embedded build lock. Context clones retain native image/lock guards even after the initial binding is dropped. Source commit remains declared; neither lock matching nor a Hello invents source attestation or creative authority. |
| Rights/Provenance — actual handshake | A real contained child and parent bind the same image/lock/request, compare both OS pipe peers with the held runner PID, then exchange strict Hello/Authorize messages in the exact random context. The original declaration deliberately does not exist: successful authentication proves no original loading or source admission. Original checking remains a subsequent independent gate. |
| Platform/Audience | Strict tagged bodies reject unknown fields/commands, invalid hash/counter/layout shapes and inconsistent observed generations. Consecutive IDs, nondecreasing receipt counters and eight outstanding commands are checked; only actual application/refusal may release an ID. Actual console rendering and the global UI/IPC/core window still need integration. |
| Platform/Audience — semantic refusal | Supported controls before text acknowledgement receive InvalidState without source motion; Status/Quit remain admissible. This differs from malformed/foreign/replayed protocol data, which ends admission. The operator's local Fault/status/quit behavior after protocol failure is still required. |
| Sound/Editor | Preserve the expected generation through callback buffering and actual ownership return. Check it when the real core applies a command, after earlier commands may change generation. A stale Pause cannot pause the newly started generation; its actual refusal trace is retained. Stop/Quit/Status remain run-wide. No PCM/order/interval/translation changes. |

Immutable Snapshot/State/Error/Command values now deserialize for observations
and strict messages. The actual Transport, Prepared and restart capabilities do
not deserialize; no observation is copied into their private state. The
controller must use the stamped command path and actual source/output objects.

Execution on the exclusive worktree target:

- Installation unit filter: **63 passed**, zero failed, in 4.84 s. Five ordinary
  ignored native/allocator fixture entries were explicitly launched by their
  parents, not counted as extra cases. Includes the contained authentication
  exchange, strict message refusal cases and actual core/callback generation tests.
- Explicit original/runtime-input integration filter: **nine passed**, zero
  failed or ignored, in 23.34 s. New current-player case succeeds with originals
  unavailable, proves cloned gate contexts retain lock guards, and rejects a
  byte-correct alternative player path and a byte-correct incorrect lock.
- Native CLI: **five passed**, zero failed, in 0.09 s. Shape/original checks
  still do not start a channel, probe an endpoint or play audio.
- Final all-target/all-feature Clippy passed with warnings denied in 44.87 s.

That is **77 distinct focused passing cases**, including the earlier cases;
reruns and fixture children are not counted again. No endpoint, real audio,
capture, human listening/usability or release was performed. Formatting,
whitespace, actionlint and changed-document relative links passed.

## Actual core, source and output controller checkpoint

The same assistant applied the same four role checklists to the
[transport/output controller](../installation-native-runtime-v1.md#real-transportoutput-controller).
It now connects the sole actual core to callback ownership, background native
operations and the retained source worker's opaque restart path.

| Lens | Finding / disposition |
| --- | --- |
| Sound/Editor | Hold the identical core during blocked open, hand it off only after a valid silent callback, retain exact source/interval cursors while paused and return that core before replacement. Controlled-backend tests cover source and interval pause, Stop, old-close gating, successful second-generation reopen and late-permit refusal after Stop. Fresh-check permits are staged through probing and consumed by the real core immediately before native opening. |
| Platform/Audience | Command records are produced before the controller schedules startup work. Stop updates the real core during blocked open/close; watchdog failure is observable without inventing a mutable snapshot while the callback owns the core. End-to-end foreground acknowledgement visibility and every aggregate numeric gate remain pending. |
| Platform/Audience — command identity | FIFO guessing is insufficient when Fault refuses a command at enqueue while a later Status gets a core sequence. Fixed-size tagged delivery records now preserve the actual tag/sequence/refusal through fill and teardown; tests prove the rejected Pause cannot steal Status's acknowledgement. Delivery overflow faults before source progress. |
| Rights/Provenance | `SourceWorker::with_runtime` holds and rechecks the actual player/lock on its checking thread, independently of original producer checking. `Controller::drive_source` issues tickets after physical close, cancels obsolete operations and stages only current completions. No wire success creates a permit or rewrites original lineage. |
| Ownership/cleanup | Controller destruction explicitly joins native output teardown while retaining the callback return receiver, then recovers actual returned ownership. A refused attachment returns a boxed Callback off the data path; no allocation/free was added to data fill. Broken native teardown still needs the enclosing process owner and remains a failed qualification. |

Preparation/ack/end messages now represent pre-transport Checking/Fault without
constructing a fake Ready core or snapshot. The actual authenticated outer worker
must route these observations, preserve local Fault/status/quit behavior and
close the source/output/IPC/process lifecycle. The controlled restart tests use
private checker permits to test orchestration; they do not substitute for the
separate real-original checking integration or device qualification.

The isolated allocation fixture now includes tagged enqueue records as well as
report overflow and ownership return. Separate delivery-overflow and faulted
enqueue/refusal tests verify source cursor and command identity independently.
Same-assistant checklist findings are not independent approval; no other craft
lens gains new authored-media scope from this change.

Final execution on the exclusive Windows worktree target:

- Installation unit filter: **70 passed**, zero failed, in 5.08 s. The five
  ordinary ignored fixtures were explicitly launched by their parent cases.
  The five controller cases use a private controlled backend; their synthetic
  checker permits test orchestration, not original-input qualification.
- Explicit original/runtime-input filter: **ten passed**, zero failed or ignored,
  in 60.31 s. The new combined source-thread case uses actual pinned player/lock
  and original inputs, drives the real controller/source ticket connection and
  checks a staged permit. It deliberately does not tick the controller into
  endpoint probing; no actual device operation is hidden in this test.
- Native CLI: **five passed**, zero failed, in 0.10 s.
- Final all-target/all-feature Clippy passed with warnings denied in 39.88 s.

That is **85 distinct focused passing cases**; previous executions and fixture
children are not extra cases. Intermediate lint/style findings and the larger
off-callback attachment-refusal representation were corrected before final
qualification. Formatting, whitespace, actionlint and changed-document links
passed. No actual endpoint, playback, capture, listening or release is claimed.

## Controller acknowledgement deadline checkpoint

The same assistant applied the Platform/Audience, Sound, Editor and
Rights/Provenance checklists to the receipt watchdog change. These are checklist
findings, not independent reviews or human approvals.

| Lens | Finding / disposition |
| --- | --- |
| Platform/Audience | The 250 ms playback watchdog did not cover a queued Resume while Paused. The controller now checks every pending command against its original receipt with a 100 ms budget. It checks before removing actual records or refusals, including teardown, so a late observation cannot erase a missed deadline. Foreground visibility and IPC remain separate unfinished obligations. |
| Sound/Editor | Timeout marks callback failure, cancels startup and requests real native close. The stalled-Paused test proves subsequent fill is silent and the source cursor stays fixed through actual core return. Late actual acknowledgements remain historical observations after Fault; they cannot clear failure or revive startup. Status/Quit remain usable. |
| Rights/Provenance | The change neither admits new originals nor bypasses player/lock checks. Tests use private controlled engineering output only; no endpoint, private media, capture or creative/release authority is inferred. |

Three new controller regressions passed: a stalled Paused Resume, a callback
acknowledgement produced before the controller reads it after the deadline, and
an expired Start receipt that opens no stream. The complete installation unit
filter passed **73 cases**, zero failed, in 7.58 s; the same five ordinary ignored
fixtures were launched by parent cases. All-target/all-feature Clippy passed
with warnings denied in 44.23 s. The explicit original/runtime-input integration
filter passed **ten cases**, zero failed or ignored, in 33.01 s; the native CLI
passed **five cases**, zero failed or ignored, in 0.11 s. This is **88 distinct
focused passing cases**, not an addition of reruns or fixture children.
Formatting, whitespace and actionlint passed. No actual endpoint, audio output,
capture, listening or release was performed.

## Remaining proof

The real transport must stay in the controller until native readiness, then
move once into the callback and return intact through the actual backend.
Primitive and controlled-output ownership/race cases above passed; complete
native session integration remains open.
Framing, peer, authentication components and scoped command application passed.
The transport/source/output controller exists; complete authenticated session
routing, parent observation ordering, global command-window accounting, console
behavior, native cancellation through the complete contained worker and all
aggregate deadlines remain open. The new pre-transport observations must be
integrated without inventing a Ready transport to acknowledge those controls.
The controller now enforces the receipt-to-controller acknowledgement budget,
including a stalled callback while Paused. The complete foreground/IPC path
still needs the same original receipt deadline, actual visibility/order proof
and rejection of late observations as qualification success. These controller
tests do not establish the end-to-end 100 ms control-acknowledgement gate.
Only after those components and full checks pass can M12b.2 be proposed for
merge. M12c's actual station/capture/listening/usability gates remain separate.
