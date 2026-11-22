# From checked originals to explicit playback

**Status: M12b.2 implementation in progress.** The merged
[M12b.1 foundation](installation-native-foundation-v1.md),
[#403](https://github.com/giodl73-repo/REEL/pull/403), supplies metadata and
callback ownership. This document maps the remaining native session boundary.
It does not qualify a playback command, terminal or actual audio endpoint.
The [M11 limits](offline-installation-v1.md) remain authoritative.

```mermaid
flowchart LR
    accTitle: Original production and native playback have different identities
    accDescr: Retained original compiler bytes and programme controls feed the existing independent checker. The same checked transport moves into the callback only after explicit Start and native readiness. The foreground exchanges bounded commands and observations, never a restart permit.
    original["Original compiler/producer bytes<br/>declaration · plan · receipt · access"] --> check["Existing checker + retained inputs<br/>no current-player substitution"]
    check --> ready["Controller owns same transport<br/>initial Ready / verified Restarting"]
    ready --> start["Explicit Start / Restart<br/>bounded exact-endpoint open"]
    start --> callback["Transfer the same transport<br/>only when native callback is ready"]
    callback --> returned["Return intact for Stop/Restart<br/>fresh check permit stays in worker"]
    returned -->|"explicit Restart + fresh checks"| check
    operator["Foreground operator<br/>exact text + line commands"] <-->|"bounded private IPC<br/>session + sequence + generation"| start
```

## Private invocation, existing programme

`reel.installation-session.v1` is a private invocation request, not another
authoring, storage or programme manifest. Its maximum encoded size is 16 KiB.

| Field | Meaning |
| --- | --- |
| `edition_id`, `passes` | Exactly one original edition; one to three passes. |
| `original` | Exact FileRefs for declaration, plan, production receipt, optional processing policy, access manifest and retained original compiler/producer bytes; explicit source/candidate/access roots. |
| `playback` | Separate retained executable FileRef, independently bound source commit/Cargo.lock context and pinned CPAL version. A declared build context is not an inferred source attestation. |
| `endpoint` | M12b.1's exact private profile/device/build/driver/configuration binding. Shape validation does not probe it. |

Never fill the original compiler identity from the current player or from the
receipt being checked. Original runtime bytes may have been produced on another
platform: retain and hash them without executing them or demanding a Windows
executable suffix. Only the actual native player/worker is an executable binding.

The source loader holds mandatory Windows file and parent-path guards using the
existing runtime protection primitive. Metadata is read under its exact pin;
providers retain the corresponding guards for source/candidate/access reads.
They reject changed roots, conflicting pins, missing/case-mismatched names,
escapes and excessive inputs. Each provider admits at most 128 held files / 64
MiB; no source lookup, cache initialization, restoration or fallback is added.
Unsupported guarding refuses rather than claiming cooperative native custody.

`SessionRequest::check_originals` is an explicit read-only source operation.
Its counts-only result distinguishes checked original runtime bytes and guarded
reads from **unchecked** playback build, host, endpoint and device playback. It
does not open the declared player, probe a device or create a success artifact.
Original checking stays within one aggregate 30 s cooperative deadline.

The CLI exposes `validate-session <private-request.json>` for shape only and
`check-session <private-request.json>` for those original checks. Neither
command is playback. Protected original file checking currently requires
Windows; unsupported platforms refuse rather than reporting protected reads.

Fresh checks still run before transport creation and restart. Once their seals
match, Prepared and Transport share the same frozen sample/text allocations;
the active session does not retain a second identical playback PCM buffer.

## Ownership before and after native readiness

The controller must retain the actual transport while native probe/open/play
is pending. An output callback can submit zeros before receiving the transport;
only explicit Start may create that stream. After native readiness, transfer
the same object once. A startup Stop can therefore update the real transport
without waiting for a blocked native call or losing the context needed for an
explicit later Restart. Late native completion cannot acquire a cancelled box.

The source-check port now owns LoadedProgramme and Prepared on a dedicated
thread for the session. Guarded load, initial preparation and fresh seal checking
share one absolute deadline of at most 30 seconds. The returned Ready transport
uses the Run origin, so setup counts toward its 120-second lifetime. Restart
tickets and permits move through bounded in-process queues; they are never JSON
or another authority inferred from an observation. Event reception checks for
cancellation/expiry again to reject a success queued just before cancellation.
The actual transport independently rejects an otherwise checked permit after
Stop or a generation change. The source port opens no endpoint and does not
qualify the distinct playback executable or host. Its destructor joins the
source thread and retained guard destruction; complete process containment is
still required to handle a blocked OS operation.

While active, M12b.1's callback owns the sole source/interval clock. Stop,
Restart, completion or failure must return that object before replacement.
Neither a private wire status nor a deserialized success flag creates a restart
permit. Prepared and the opaque ticket/permit exchange stay in one native
worker; original checking and native open/drop stay off the control/callback
threads. A failed native cleanup remains a failed profile, not a timer success.

## Controller and operator obligations

The existing contained process runner now offers an explicit observer port.
It obtains the primary worker PID and peak working set from its own held process
handle, rather than trusting a child-written PID or message. `exit_observed`
means the runner observed that process handle signaled; a separate `Reaped`
event follows actual job shutdown and descendant-handle barriers. An observer
error cancels through the existing tree cleanup before the call returns. The
original `run_quiet` callers keep their existing path and incur no new process
memory queries. No second launcher, cache or telemetry store was added.

These observations supply the ownership facts needed for private-pipe peer
verification; that integration is still pending. They measure only the primary
process's peak working set. A zero/failed observation cannot qualify memory use,
and the session must still measure its foreground process and enforce the
combined 512 MiB envelope. Job committed-memory limits are a different metric.
The observer runs on the existing runner thread, whose 250 ms monitor cadence
cannot replace the session's 20 ms controller or 100 ms acknowledgement rules.

The Windows output port now implements explicit probe/open/close on one serial
backend thread. Construction opens no endpoint. Real opens resolve the bound
WASAPI endpoint again, request 480 stereo float32 frames at 48 kHz and check the
reported period plus every actual callback's whole-frame/2048-frame bound.
`play()` returning is insufficient: a valid silent callback must establish
readiness before the controller can attach the actual transport. Cancellation
also travels with the sink, so a late open cannot acquire it.

The application data callback records fixed-size atomic observations and actual
source-frame submissions, excluding pause, interval, fault and padding silence.
Its QPC clock shares the pinned CPAL WASAPI timestamp domain. A predicted output
timestamp remains a prediction. Closing reports completion only after the actual
stream is dropped; the port's destructor joins its backend thread. This can
block on a broken driver and therefore belongs inside the still-required owned
worker process. This port alone is not a contained foreground runtime.

Controlled-backend tests compare every float through variable callback sizes,
pauses in both source and interval, cancellation and oversized buffers. A separate
isolated allocator fixture covers the complete application data-fill wrapper,
including real report overflow and ownership return. It does not establish the
allocation behavior of CPAL internals, driver/error callbacks or teardown.
At 120 seconds the actual core stops; only Close may use the two-second cleanup
tail. Probe/Open cannot extend playback into that tail.

The session's 120 s lifetime starts at invocation, including preparation and
pauses. Initial checks share a 30 s budget; Start has a 2 s aggregate bound and
Restart has 35 s including old close, checking, probe and open. Stage limits do
not reset those aggregates. The 20 ms controller tick, 100 ms acknowledgement,
250 ms watchdog and 2 s cleanup limits remain unchanged.

Console/IPC work must not block that controller. The operator needs the actual
UTF-8 terminal geometry and exact original text before Ready/Start. Standard
line editing, echo and EOF/Ctrl+C must work while the checked transcript remains
visible above the status/input area. No raw terminal mode or text normalization.
Any proposed scrolling/display mechanism needs an actual Windows console test;
a simulated screen alone cannot qualify it.

The core's 256 diagnostic entries and callback's 256 transient report slots
serve different purposes. Native observations must be bounded too: retain
explicit aggregates and a declared sample of boundary/control records, without
claiming that a sampled timeline contains every callback. Physical capture
and device measurements remain M12c.

## Requirement/test map

One assistant applies Platform/Audience, Editor, Sound and Rights/Provenance
checklists. These are review lenses, not independent reviewers or approvals.

| Obligation | Named proof / status |
| --- | --- |
| Original and player identities remain separate | `native_original_check_preserves_producer_identity_separately_from_player`: real original programme checks, including a different CLI executable, pass with an unavailable declared player; a correctly pinned replacement producer still fails the original receipt relationship. Passed locally. |
| Guarded original reads and strict bounded invocation | `held_original_input_denies_mutation_and_rename_without_executable_requirement`, request-shape/cancellation/deadline and CLI cases. Passed locally. |
| Startup Stop retains the actual session | Native-ready handoff, Stop before first owned fill and cancelled blocked-open controlled-backend cases passed locally. Foreground/contained-process integration remains pending. |
| Callback teardown returns pending ownership | Drop-before-first-fill, queued Stop, completion, preempted writer and racing return cases passed locally. Actual native error/close integration remains pending. |
| Exact private wire context | Foreign run/sequence/generation, oversize/unknown/truncated frames, queue overflow and closed-peer cases required; pending. |
| Aggregate clocks and source verification cancellation | Source port shares the Run origin and one initial/fresh-check deadline; queued cancellation/expiry and real original rechecks passed locally. Complete foreground Start/Restart aggregates, Stop/Quit integration and blocked-operation containment remain pending. |
| Exact persistent text and line commands | Actual Windows console, Spanish cells, paste/scroll/resize, ordinary EOF/Ctrl+C, blocked I/O and bounded cleanup; pending. |
| Real contained lifecycle and unchanged PCM | Controlled native output port passed source/interval sample oracles, allocation/free, cancelled handoff, oversized period/buffer and actual mock-stream close tests. Actual contained worker/driver lifecycle remains pending. |

No complete M12b.2, actual playback, device/listening/usability or release claim
follows from these source/output ports. M13 still depends on full M12.

Local source/output checkpoint: 41 unit cases, five CLI cases and eight
explicit original-input cases passed: **54 distinct focused cases**. The
original-input invocation ran with zero ignored cases; the three ordinary ignored
unit fixture entries are explicitly launched by their parent tests. These counts
do not include duplicate reruns. The real source-worker integration retains
original file guards through Ready/rechecks, releases them at actual teardown,
and proves that a genuine later recheck permit cannot revive Stop.

At ownership return, an atomic writer/return handshake prevents a newly accepted
command from being stranded in the old callback queue. A busy writer defers
the move without blocking `fill`; late commands already accepted move in the
actual transport's core queue. Later sends receive a typed, nonfatal returned-
owner refusal so the controller can apply them locally. Teardown also returns
the same box before a first callback. Only teardown, off `fill`, may wait for a
preempted writer; its complete native cleanup still needs the contained worker.

Each report retains at most eight actual command records/refusals, selecting
the first command record for a sequence rather than a later media event with
the same sequence. Report capacity is checked before copying source frames:
overflow silence cannot advance the submitted-source cursor. The isolated
allocator fixture exercises this actual overflow with one-frame callbacks.
