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

### First hosted cycle and current-base refresh

[CI 45036548394](https://github.com/giodl73-repo/REEL/actions/runs/45036548394)
completed in 19m32s. Both OS Clippy and ordinary test jobs passed; downloaded
job logs confirmed the new callback/metadata cases and all three CLI cases on
each OS, plus the actual owned-process case on Windows.
[Assembly/source qualification](https://github.com/giodl73-repo/REEL/actions/runs/45036548357)
passed in 5m14s.

The overall CI result was **failed**: the unchanged reel-assets
`installed_runtime_job_retains_output_and_rejects_changed_tools_and_failures`
case exceeded its existing 30 s synthetic PowerShell deadline. Running that
exact case locally passed in 4.88s, adding one distinct investigation case to
the 24 focused cases above. No source, deadline or gate was relaxed.

Meanwhile documentation-only main PR #404 advanced the base to `fb8ac7b`.
After the complete first cycle, rebase onto that base and require a fresh full
qualification before merge. First-cycle component passes are not an overall
current-base pass. The PR records the exact final head and result; no run was
cancelled and no actual endpoint was used.

### Repeated shell-fixture failure

The second current-base cycle,
[CI 45038530702](https://github.com/giodl73-repo/REEL/actions/runs/45038530702),
completed in 14m38s and also failed only the same synthetic PowerShell case. Both OS ordinary/Clippy
jobs and [assembly/source](https://github.com/giodl73-repo/REEL/actions/runs/45038530707)
passed. Repeating the same full cycle again without addressing that dependency
would not resolve the gate.

Replace only that test's installed executable with the actual native Rust test
image, outside the job workspace. Three ignored child fixtures explicitly
exercise successful output, nonzero failure and a stalled process. The parent
still ingests/registers the executable, verifies retained output, rejects a
changed hash and absent path, and exercises the existing 30 s success / 1 s
timeout controls. No production runner, admission rule, timeout or check is
weakened. These child entries are invoked by their parent, not counted as extra
ordinary passes. The focused parent passed in 4.45s and affected-crate
warnings-denied Clippy passed. Full all-target asset validation then passed:
236 library cases (including the installed-runtime parent under parallel load),
41 binary-unit cases and 61 integration cases, **338 distinct passes**. The
three child fixtures are ordinary ignored entries but are invoked by the parent.
The library suite completed in 18.60s. After tightening the parent's output
oracle to the exact declared text and its failure oracle to exit 101, the final
focused parent passed again in 4.49s and affected-crate Clippy in 3.67s. A new
full current-base hosted cycle must qualify this exact correction before merge.
