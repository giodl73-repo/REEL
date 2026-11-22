# M12b.1: native enrollment and ownership foundation

Status: local qualification in progress, based on merged M12a `f6d079c` / [#400](https://github.com/giodl73-repo/REEL/pull/400).
One assistant applied the role checklists; no independent reviewer, device
acceptance, voice permission or release approval is claimed.

The [illustrated contract](../installation-native-foundation-v1.md) narrows this
PR to explicit metadata observation and callback ownership. M12b.2 retains the
actual operator/native playback loop; M12c retains station measurements. This
split preserves the full M11/M12 exit requirements rather than qualifying a
player from a port or a probe.

## Requirements and tests

| Requirement | Independent oracle / test |
| --- | --- |
| Read-only validation | `installation_native_cli::binding_shape_validation_is_private_and_needs_no_device`: fictional explicit ID validates with no query; stdout omits IDs/driver details and declares no probe/playback. |
| Strict input / publication | CLI cases reject unknown fields, oversized controls, aliases, wrong runtime pins and existing output; preserve original bytes and create no successful profile on refusal. |
| One callback owner | `callback_preserves_pcm_and_returns_the_same_restart_context`: hand-derived sample position, frozen paused cursor, same transport/text and usable original restart ticket after ownership return. |
| Boundary ordering / overflow | `callback_stop_precedes_first_opened_and_fatal_queues_return_ownership`: Stop defeats a first Opened notice; full command/report channels fault and return ownership without emitting substituted data. |
| Callback allocation | `callback_handoff_and_overflow_do_not_allocate_or_free`: isolated fixture instruments actual allocation/deallocation calls during fill and ownership return. Its fixture is explicitly invoked; an ordinary ignored fixture is not a pass. |
| Process ownership | Windows `timeout_and_cancellation_reap_actual_owned_process_before_return`: wait for a real child PID, hold its process handle, cancel/deadline it, then assert process-signaled state and absence of its late output. |
| Actual device | Not executed as a substitute for enrollment. Endpoint observation and buffer/playback flags retain their declared limited scope. M12c remains required. |

## Role findings

| Lens | Finding / disposition |
| --- | --- |
| Platform/Audience | Validation cannot open a device; the metadata tool has no Run command. Full terminal controls and usable text presentation are still M12b.2, not inferred from JSON. |
| Editor | The same transport and its cursors move across the callback boundary. No restarted clock, implicit source seek, repeated frame or late-generation revival is accepted as a handoff. |
| Sound | No stream/gain/capture operation is exposed in this first native component. Declared 480-frame configuration is not proof of actual buffer behavior or listening quality. |
| Rights/Provenance | Exact executable protection and explicit endpoint selection; private observations and sanitized counts only on stdout. No default fallback, invented driver version, source import, cache admission, media upload or release. New optional dependencies have pinned scope and original direct-license texts. |

Animation, source wording/vocal adaptation, story alteration and score/
reconstruction lenses are unused: no visual or authored-media content changes.

## Execution record

Exclusive Cargo target: `F:/tmp/reel-builds/media-m12-native-isolated`. Local
Windows 11, Rust 1.98.0; dev/test debug symbols disabled, assertions intact.
All-target/all-feature Clippy passed with warnings denied (2m08s). The focused
unit run passed 15 cases (3.49s); its two ordinary ignored fixture entries were
explicitly launched by parent tests, not counted as additional independent
passes. All three metadata CLI cases passed (1.40s). All six original-input
installation cases passed explicitly with `--ignored --test-threads=1` (28.65s),
with none ignored in that invocation. This is 24 distinct focused cases.

The final platform review added an actual native-architecture check to exclude
ARM64 emulation as well as Windows Server; focused unit/CLI tests and Clippy
passed again against that final source: Clippy in 1m14s, all 15 unit cases in
3.34s and all three CLI cases in 1.43s. The six original-input cases exercise
unchanged checking/transport code; the final change touches only the native OS
query. Formatting, whitespace, actionlint and relative documentation links
passed. Hosted qualification remains pending and will be recorded on the PR
against the exact tested head before merge.
No actual endpoint, playback or capture was started.

Commands (all use the exclusive target above and `-j2`):

```text
cargo clippy --locked --all-targets --all-features -j2 -- -D warnings
cargo test --locked --all-features --lib spoken_programme::installation:: -j2 -- --test-threads=1
cargo test --locked --all-features --test installation_native_cli -j2
cargo test --locked --all-features --test spoken_programme_media installation:: -j2 -- --ignored --test-threads=1
```

The isolated workflow SDK's locked/offline Windows-target metadata resolution
also passed. No SDK lockfile, workflow implementation or active lane changed.
