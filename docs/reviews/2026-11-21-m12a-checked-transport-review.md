# M12a: checked transport core and remaining output gates

Status: implementation under local qualification, based on merged M11
[#398](https://github.com/giodl73-repo/REEL/pull/398), `81be1db`.
One assistant applied the repository role checklists. This is not independent
review, human acceptance or permission to exhibit a programme.

## Why the M12 work is bounded into three PR gates

M11 combines three different proofs. Keep each reviewable without lowering its
exit criteria:

| Gate | Bounded change | Exit / dependency |
| --- | --- | --- |
| M12a — this change | Original-input preparation and platform-neutral transport, including exact source/interval cursors, finite loops, command order, restart rechecks and bounded refusal states | Original-source integration tests and deterministic sink/clock tests on Linux and Windows. No native device capability is granted. |
| M12b | Explicit local CLI, private runtime/device binding and contained Windows CPAL/WASAPI worker; terminal geometry, endpoint configuration, cancellation and generation exchange | Depends on merged M12a. Must meet M11's worker/resource/clock/control limits and actual native error/cleanup tests. No background startup, server, default-device fallback or new asset store. |
| M12c | Exact station enrollment and bounded device/capture demonstration | Depends on M12b. Measure I08/I09 on one explicitly selected station, including disconnect and the declared capture oracles; record listening/usability/authority separately. |

M12 remains **open** until all three gates have their own evidence. M13's
dependency is the whole M12 checkpoint, not merely the portable core.
Device evidence must name its observation point: software loopback does not
prove physical loudspeaker/DAC behavior. M12c must retain that distinction when
evaluating M11's physical-output limits, leaving any unmeasured layer open.

## Reuse and implementation boundaries

`spoken_programme::installation::prepare` uses the existing independent checker,
original `CheckRequest`, `verify_access`, original providers and shared strict
PCM24 WAVE parser. It admits a single selected edition only after all original
checks; it freezes its checked PCM/text in memory. `Selection` is a Rust API
argument, not a new programme/asset schema. No CPAL dependency is added in M12a.

`Prepared::transport` rechecks originals before returning Ready.
`Prepared::restart_permit` checks originals without borrowing the mutable
transport. Thread-safe providers let a checker run off the controller; the
controller can still process Stop/Quit while it runs. A private restart ticket
binds both the process-local session and generation, even when two sessions use
identical source bytes. `Transport::accept_restart` consumes the sealed,
fixed-size permit once. A late, foreign-session or duplicate result,
changed original or elapsed deadline cannot revive a stopped/faulted session.
Generic output notices are transport
inputs, not verified device claims; M12b owns their real runtime binding.

The transport uses integer source and 12000-frame interval cursors. Commands
precede media boundaries; already observed backend/deadline/watchdog faults
dominate commands. Old-generation callbacks produce silence without advancing
the new run. State, queue and trace storage are bounded; callback fill performs
no allocation, locking, I/O, hashing, device query or checker invocation.

The text profile preserves exact UTF-8 with precomposed Spanish and bounded
single-cell Latin/punctuation. It rejects unsupported/wide/combining/control
characters rather than changing the original text. Actual terminal size,
display and screen-reader behavior are M12b/c evidence, not a string-test pass.

## Requirement-to-test map

| M11 requirement | M12a planned/executed test owner | Still open |
| --- | --- | --- |
| I01 original PCM/access and refusal before output | `installation::checked_selection_requires_original_pcm_and_access`; `selection_deadline_and_cancellation_refuse_without_fallback`; `nonquiet_seam_and_self_rehashed_candidate_refuse` | Native preparation/CLI must preserve this source boundary. |
| I02 exact samples, passes and interval | `transport::tests::exact_pcm24_conversion_and_finite_passes`; `installation::exact_checked_pcm_reaches_finite_transport_without_device` | Actual output conversion/configuration and capture. |
| I03 pause at exact source/interval cursor | `transport::tests::pause_preserves_source_and_interval_cursors` | Measured acknowledgement / physical quiescence. |
| I04 ordering and old generations | `transport::tests::command_order_and_old_generation_refusal`; `startup_stop_wins_and_lifetime_cannot_be_restarted` | Real bounded IPC and cancelled native startup. |
| I05 restart originals and deadlines | `transport::tests::restart_requires_fresh_checks_and_rejects_late_cancelled_results`; `installation::restart_rechecks_same_originals_and_stale_input_faults` | Worker close/reopen under M11's actual 35-second aggregate. |
| I05 responsive checking handoff | `installation::stop_remains_responsive_while_original_restart_check_is_in_flight` blocks the actual checker on another thread, stops the transport, then rejects its late sealed permit | Native controller scheduling and measured acknowledgement remain M12b/c. |
| I05 session separation | `transport::tests::identical_source_sessions_and_duplicate_permits_cannot_cross` | Tickets are in-process values; M12b must preserve its own real worker/session boundary, not deserialize a success report as authority. |
| I06 bounds, faults and lifetime | `transport::tests::limits_fault_cleanup_and_no_auto_resume`; `bounded_queues_and_command_refusals` | Actual process cleanup, EOF/Ctrl+C and physical disconnect. A typed close effect is not process termination evidence. |
| I07 exact text without audio | `installation::tests::exact_text_access_without_audio`; original-access integration case | Actual terminal/display/usability. |
| I08/I09 | Not executed by M12a | Explicit WASAPI endpoint, native stream, real capture and all measured device limits; no successful hardware skip. |

## Role checklist

| Lens | Finding / disposition |
| --- | --- |
| Platform/Audience | Original text remains accessible in Ready/Paused/Stopped without sound. API text validation does not claim a rendered accessible station; terminal and real device remain later gates. |
| Editor | Whole-edition order remains upstream. Exact finite passes and explicit silence prevent frame-padding drift; Pause never follows elapsed wall time through the source. |
| Sound | Reuse independently checked PCM; convert integer PCM24 exactly to f32 and refuse nonquiet head/tail instead of inventing fades. Synthetic signals are not speech/listening evidence. |
| Rights/Provenance | Original names/pins and production identities remain upstream. Summaries expose counts and fixed false device/listening/authority/release flags. No real project media, capture, cache admission, voice service or public release. |

Animation, story alteration, vocal adaptation, score and reconstruction lenses
are unused: no visual output, wording, translation, performance or score changes.
The current core does not start an endpoint or satisfy M12's player/device exit.

## Validation record

Initial local build uses only `F:/tmp/reel-builds/media-m12-isolated`, with dev/test
debug symbols disabled and application assertions intact. Unit/integration
results, warnings-denied Clippy, formatting, actionlint and hosted evidence will
be recorded after execution. Existing ordinary media, reading, lesson, companion
and required Rust/FFmpeg checks remain enrolled. No timeout is relaxed.

Earlier checkpoints: eight initial unit cases passed, then nine cases passed
after isolating the PCM24 rail oracle. Six original-input cases passed in
29.79s, including the real concurrent-check/Stop handoff. The current
session-ticket refinement and relevant regressions are rerunning; those earlier
passes do not qualify a later source tree by themselves.

The first end-to-end fixture run refused all six cases during fixture setup:
full-scale test rails violated the existing producer peak profile. The fixture
now uses small controlled values; raw signed rails remain a separate conversion
unit test with literal IEEE-754 answers. The producer limit was neither raised
nor bypassed. That failed setup run is not counted as passing qualification.

### Frozen-code focused checkpoint

The final session-ticket code passed all ten unit cases (0.18s) and all six
explicit original-input cases (45.33s), with zero failed/ignored cases. The
original checker was actually blocked on a second thread while Stop completed;
its subsequent permit could not revive the stopped transport. Literal float
bit-pattern checks cover conversion rails separately and small admitted values
through actual checked WAVE input.

All twenty existing declaration/compiler cases passed (3.47s). Ordinary media
regressions and final warnings-denied Clippy are still running at this checkpoint.

The subsequent ordinary media run passed all 24 cases (185.53s); its 28 expected
ignores are not counted as executed cases. The six installation cases ran
separately and explicitly above. This is **60 distinct passing local tests**
(10 unit + 6 installation + 20 declaration/compiler + 24 ordinary media), not
including repeated earlier checkpoints. Final Clippy/hosted checks remain to
be recorded.
