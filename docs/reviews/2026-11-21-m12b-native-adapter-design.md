# M12b native adapter: source-backed preparation

Status: preparatory design only, based on merged M12a `f6d079c` / [#400](https://github.com/giodl73-repo/REEL/pull/400).
M12a passed full current-base CI in 16m40s; its merge tree matches tested head
d844abe. M12b implementation and native qualification remain separate. No endpoint probe, stream, playback, capture, cache or private
project media has been opened by this design work. A pending parent's checks
cannot qualify this gate.

## Concrete findings in pinned CPAL 0.18.2

Inspected the downloaded package's exact source, not the current development
branch. Versioned upstream links allow a reviewer to repeat the inspection.

| Finding | Consequence for the adapter |
| --- | --- |
| WASAPI `DeviceHandle::Specific` comes from enumerated `IMMDevice`; `default_device_monitor` returns `None` for it. | Resolve the explicitly bound `DeviceId` through the WASAPI host's device enumeration, verify output capability and its configuration, and retain that specific device. Never use `default_output_device` or follow a default-role alias. |
| `supported_formats` calls `ensure_future_audio_client(None)`; specific-device activation is synchronous. A stream activation timeout does not bound every native call. | All native probing and stream creation must run in the owned child, with the controller enforcing M11's deadline. A library timeout argument alone is insufficient. |
| `Stream::drop` sends Terminate and joins its thread; the native event wait uses an infinite Windows wait. | Keep close/drop out of the foreground controller. Bound and own the complete child tree; a stalled driver must not turn a controller's Stop into an unbounded join. Measure cleanup separately. |
| WASAPI's `description().driver` is populated from the interface name. | It is not an observed driver **version**. M12b needs a separately observed Windows endpoint/driver binding; do not fill a version field with a friendly label. |
| The actual stream obtains its buffer size from `GetBufferSize`, after initialization. | Request 480 frames, but verify the effective stream/callback limits. Refuse an oversized callback and silence/terminate the session; do not assume the requested size was honored. |
| An output device can be opened as an input for WASAPI loopback. | The playback adapter must expose no input/capture operation. M12c's explicitly enrolled capture remains separate and names its actual observation point. |

Source: [WASAPI device implementation](https://docs.rs/crate/cpal/0.18.2/source/src/host/wasapi/device.rs),
[WASAPI stream lifecycle](https://docs.rs/crate/cpal/0.18.2/source/src/host/wasapi/stream.rs),
[host lookup contract](https://docs.rs/cpal/0.18.2/cpal/traits/trait.HostTrait.html).
These are API/source findings, not measured device behavior.

## Ownership and process boundary

Reuse `media_runtime::BoundTool` for exact executable identity/read guards and
the existing Windows `reel_assets::jobs::run_quiet` contained child primitive.
The foreground controller owns one child and one private, bounded session
workspace. This is an explicitly started playback session, not a cache job,
asset admission, daemon or service. Do not fabricate a Tig checkpoint or import
another project's producer bindings.

The child must remain quiet; bounded private status and command exchange carries
only the current session's declared records. No arbitrary path/resource routes,
network listener, environment tool discovery or unlimited diagnostic files.
Hold and verify source/runtime bytes before launch. The worker accepts the exact
selected snapshot and runtime/device context, not a freely deserialized success
summary as original-check authority.

The application data callback owns the portable transport while active. Its
control/status exchange must be bounded and nonblocking; do not add a mutex,
file poll, checker, serialization, allocation or console write inside it. Pause
continues callbacks with zeros. Restart first closes the old native stream,
retains the same transport/session context, checks originals off the controller,
and consumes its matching ticket/permit before opening the new stream.

If a proposed ownership channel cannot return transport ownership for restart
without violating that callback rule, stop that design and choose a proven
bounded transfer mechanism. Do not duplicate the source clock in a competing
controller or recreate a session just to avoid preserving its generation.

The native IPC boundary needs its own actual run/worker identity and held
context. M12a tickets are opaque in-process values; serializing a portable report
does not create a native permit. Late/foreign messages cannot open an endpoint
after Stop/Quit, replace the source or reset the lifetime deadline.

## Test obligations before native qualification

| Requirement | Required proof |
| --- | --- |
| Explicit startup | Separate read-only preparation/checking from explicit foreground Run. Checking and Ready create no stream; Start is required for nonzero output. Native-only operations on unsupported platforms refuse honestly. |
| Binding | Exact runtime/commit/lockfile/OS build plus observed endpoint ID, actual configuration and driver version. Reject changed/missing/unsupported values and any default-device fallback. |
| Ownership | Synthetic contained-worker cases exercise stalled probe/open/drop, invalid status, bounded exchange, cancellation, process exit and descendant cleanup. Use real owned process barriers, not a close-effect assertion as cleanup evidence. |
| Callback | Reuse M12a sample/interval/cursor oracles through the actual adapter boundary. Prove the chosen handoff is bounded and performs no callback allocation/locks/I/O; reject oversized buffers and queue/trace overflow. |
| Restart | Stop/Quit while checking/opening must complete without waiting for verification. Old generations, foreign sessions and duplicate results never reopen. Actual native close/reopen retains the original 35 s aggregate and 120 s session lifetime. |
| Text/operator | Verify actual UTF-8 terminal geometry and exact text before playback; no truncation. Commands, EOF and Ctrl+C have visible state and bounded cleanup. |
| Evidence | Native adapter execution, device/capture measurements, listening/usability and authority remain separate. Missing enrollment is nonexecution, never a green hardware skip. |

M12c still owes the actual isolated station and all numeric measurements in M11.
Software loopback cannot establish physical loudspeaker/DAC behavior; retain
that gap explicitly unless a capture path measures it. No new default device,
system volume or endpoint settings may be changed as implicit test setup.

## Checklist disposition

One assistant applied Platform/Audience, Editor, Sound and Rights/Provenance
checklists to these boundaries; no independent review or approval is claimed.
Animation remains unused for this audio/text profile. The source findings above
are requirements for implementation, not reasons to widen M11's limits or to
claim a qualified installation from M12a's portable tests.


