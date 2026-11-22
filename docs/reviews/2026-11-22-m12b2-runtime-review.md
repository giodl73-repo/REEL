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

## Remaining proof

The real transport must stay in the controller until native readiness, then
move once into the callback and return intact through the actual backend.
Primitive and controlled-output ownership/race cases above passed; complete
native session integration remains open.
Wire nonce/sequence/generation,
closed-peer and bounded-I/O cancellation proof, actual console behavior,
native open/drop/recheck cancellation and all aggregate deadlines remain open.
Only after those components and full checks pass can M12b.2 be proposed for
merge. M12c's actual station/capture/listening/usability gates remain separate.
