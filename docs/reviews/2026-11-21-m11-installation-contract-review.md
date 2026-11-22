# M11 installation contract: role checklist and M12 test obligations

Status: contract review, based on merged M10 `d7602b2` ([#395](https://github.com/giodl73-repo/REEL/pull/395)). One assistant
applied the repository checklists; these are not independent reviewers or human
approvals. No player, endpoint probe, sound output, capture or cache operation
ran. M11 changes documentation only; M12 owns implementation and device evidence.

## Existing owners and proposed gap

The [contract](../offline-installation-v1.md) preserves the original spoken
checker, access verifier, `InputFiles`/`FileRef` and retained source/runtime
versions. Repository search found native renderers and browser editorial
audition surfaces, not an installation transport. CPAL 0.18.2 / WASAPI is a
proposed narrow output adapter, not an existing qualified REEL feature.

The first profile is audio plus static original text. Animation is deferred
explicitly; M10's SVG clock/worker is not relabeled realtime playback. Other
platforms, unlimited loops, background operation, deployment and remote control
are outside this contract.

## Review findings and dispositions

| Checklist | Finding → contract requirement |
| --- | --- |
| [Platform/Audience](../../.roles/platform-audience.md) | Name one Win11 x64 WASAPI output by private stable ID/configuration, not a default-device heuristic. Exact transcript and line commands remain available with audio off. Terminal fit and actual usability are separate; no phone/kiosk/accessibility certification claim. |
| [Editor](../../.roles/editor.md) | Whole-edition order remains unchanged. Finite passes and exact 12000-frame inter-pass interval are authored transport behavior. Pause retains the precise source/interval cursor; Restart explicitly begins pass 1. A loop is not a branching narrative. |
| [Sound](../../.roles/sound-designer.md) | Preserve checked PCM; only exact PCM24-to-f32 conversion. Require quiet head/tail rather than inventing fades. Distinguish submitted, predicted and physically captured output; define measurable seam/drift/latency gates. No automatic gain or source repair. |
| [Rights/Provenance](../../.roles/rights-provenance-steward.md) | Source, access, runtime and endpoint bindings remain exact/private. No input capture, network, fallback, restoration, auto-restart or public identity-bearing report. Controlled device capture requires an isolated test endpoint and retained private evidence. Technical playback is not voice consent, exhibition permission, listening approval or release. |
| [Animation](../../.roles/animation-director.md) | Read for scope: no animated display in this initial profile. Visual companions retain their separate M10 derivation and render checks; their existence cannot qualify live presentation. |

Story, vocal adaptation, score/arrangement and music reconstruction are not used:
M11 changes neither authored wording/order, performance, translation, score nor
recording. Real speech/listening and project adoption remain open.

## Exact tests required in M12

Names below are required planned tests, **not executed tests**. Deterministic
tests use injected ports and clocks on Linux and Windows. Actual WASAPI/device
measurement is separately enrolled; missing hardware cannot count as a pass.

| ID / planned test | Independent positive oracle | Required refusal / failure |
| --- | --- | --- |
| I01 `checked_selection_requires_original_pcm_and_access` | Re-run original checker/access verification; selected hash/length and transcript match original objects. No output stream exists in Checking/Ready. | Rehashed false receipt, stale text, wrong edition, missing original data, unsupported PCM and expired 30 s check deadline; no source substitution/provider fallback. |
| I02 `exact_pcm24_conversion_and_finite_passes` | Hand-calculated negative rail, -1, 0, 1, positive rail map exactly to f32. Compare every submitted channel/frame across 1/2/3 passes and exactly 12000 added zeros between passes. | Zero/four passes, excessive source duration, 44.1 kHz, mono, wrong format, nonquiet 4800-frame head/tail or partial stereo callback. |
| I03 `pause_preserves_source_and_interval_cursors` | Fake clock advances arbitrarily while Paused; both cursors remain fixed. Resume starts at the exact next unsent source/interval frame. | Invalid pause/resume states leave cursors unchanged; wall time cannot skip samples. |
| I04 `command_order_and_old_generation_refusal` | Command sequence plus media/error trace gives identical states, frame spans and generations. Stop at pass end prevents another pass; Fault dominates Restart. | Queue overflow faults; invalid commands never move state; late old-generation callbacks cannot advance the new run. |
| I05 `restart_rechecks_and_returns_to_pass_one` | Restart closes old stream, verifies same original retained subjects/runtime/endpoint and starts source frame 0/pass 1 within 35 s. Starting acknowledges before a ≤2 s open; Stop/Quit cancels either operation. | Changed runtime/endpoint/media/text, recheck deadline, late cancelled worker response or open failure → terminal Fault; no default retry. |
| I06 `limits_fault_cleanup_and_no_auto_resume` | Inject 250 ms callback stall, backend invalidation, 120 s lifetime, Stop/EOF/Ctrl+C; sink closes, bounded diagnostic persists and new process starts Ready. | No progress inferred during stall; replug cannot resume; 8-entry command / 256-entry diagnostic bounds enforced. A blocked driver fails the cleanup gate. |
| I07 `exact_text_access_without_audio` | Original EN/ES UTF-8 bytes shown before Start and while paused/stopped; engineering signal text retains its label. | Missing/overlong transcript, escape sequences, unsupported terminal size/layout or silent truncation. No transcript synthesized from a signal. |
| I08 `explicit_wasapi_endpoint_and_configuration` | Windows adapter resolves only bound DeviceId/config and runtime; 48 kHz stereo f32, callback ≤2048 frames. | Missing/changed device, unsupported stream, oversized callback, backend errors, default-device substitution or unbounded stream-open behavior. Explicit enrollment required. |
| I09 `bounded_device_session_and_capture_measurements` | One selected real station performs all listed controls, loops and recovery refusals. Independently analyze isolated controlled capture: interval 250±50 ms, span error ≤20 ms, Start ≤2 s / Restart ≤35 s, control ack ≤100 ms, quiescence ≤250 ms, deadline exit ≤2 s. | Disconnect/reconfiguration, missed numeric gate or missing exact capture/config evidence leaves device qualification failed/unperformed; fake sink cannot substitute. |

## Proof and evidence procedure

1. Pin the actual merged M10/M11 commits, source fixture revision, REEL binary
   hash/length, lockfile, OS build, CPAL resolution and private endpoint/driver
   binding. Define expected marker offsets independently of playback output.
2. Run all deterministic I01–I07 tests on both operating systems. Assert failing
   relationships/states, not merely nonzero status. Validate that rejected inputs
   never open an endpoint or create a success report.
3. On explicitly enrolled Windows hardware, run I08 and I09. Keep exact raw
   capture and detailed endpoint/source identities in existing private evidence
   storage, not Git or portable summaries. Record measured values, not just flags.
4. Record native/runtime, device/capture, human listening/usability and authority
   separately. A missing layer remains open. Do not increase timeouts or relax
   numeric gates to relabel a failure without a separately reviewed contract
   change.

M11's own exit consists of complete normal/error examples, source-backed runtime
choice, role checklist, numeric limits, test mapping and valid links. It starts
no player. M12's exit additionally requires implementation and measured bounded
device demonstration; unattended reliability and project adoption remain out
of scope even after a successful short run.
