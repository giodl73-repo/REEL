# Versioned streaming film intent

`reel.workflow-film-render.v2` adds an explicit `streaming` envelope. Legacy
request v1 serialization omits that field and keeps its original limits. V1
cannot carry a streaming envelope, and v2 cannot omit it. The envelope schema is
`reel.workflow-film-streaming.v1`; it declares total input bytes, maximum input
object bytes and provider verification milliseconds. Exact request bytes remain
part of the packet's task identity. No request/plan upgrade is implicit.

V2 planning ceilings are 64 GiB total/per-object inputs, 300,000 milliseconds
provider verification, 18,000 seconds execution, 16 GiB output, 64 GiB workspace
and 1 TiB aggregate raw RGB. Memory remains bounded to 64 MiB–16 GiB, processes to
2–32, frames to one million, and samples to 384,000 × 18,000. Object input limits
must fit the total; actual declared input totals and each object must fit their
limits. Declared output ceilings, not only eventual output sizes, must fit the
output budget. Checked arithmetic refuses overflow. These ceilings are authoring
bounds, not measured hardware capacity or production qualification.

The E7 test uses the locally observed receipt's 7,936,104,893 input-byte budget,
five hours and 32 GiB workspace. It declares an 8 GiB master and 1 GiB review plus
the 65,536-byte delivery receipt and two explicit 65,536-byte workflow execution
controls. The latter require 131,072 bytes beyond the receipt's original three-
output total of 9,663,741,952. They are not hidden overhead. The test constructs
large output identity metadata without allocating or authenticating those media
bytes. Actual E7's 600-resource closure remains outside the flat 128-input task
contract and needs explicit bounded composition/lifecycle admission.

## Local inspection

```text
reel-workflow check-film-intent PACKET CONTROL_BINDINGS
```

`CONTROL_BINDINGS` maps only the exact logical request, job and production
control identities to existing local files. Their pinned content and metadata
closure are checked. Media and installed programs need not be present and are
not opened. The JSON report contains budget/intent metadata and explicit false
flags for media verification, snapshot authentication, execution, production
admission and native-v2 qualification. It neither writes a new artifact nor
changes a task, selection, claim or approval. These are private local controls.

## Native boundary

`film_inputs::Inputs::prepare_streamed` is a preparatory borrowed-file port. It
requires the checked bound's exact task key, complete declared aliases, matching
program pins and a live admitted/retained native input authority. It forwards
the declared input object/total/verification bounds to the existing guarded
Tig provider cache; it adds no new cache engine or fallback. The original
`Inputs::prepare` retains its 512 MiB/30-second policy. Neither port grants
execution, workspace capacity, output custody, retention or completion.

Current owned production, candidate checking and retained movie verification
explicitly refuse v2 with
`Unsupported/film-streaming-native-not-qualified`. This is intentional pending
the remaining required ports: versioned snapshot/projection admission,
workspace/capacity profiles, response and
historical output budgets, bounded composition, and real-size interruption/
retained-owner qualification. Provider-cache verification time does not by
itself bound every earlier native reopen or the complete job. The v2 input port
has authority/refusal tests, not multi-GB native-read qualification.

The subsequent [contained-process port](2026-11-19-streaming-film-process.md)
carries the envelope into versioned child requests, distinct input/output hash
allowances and file-backed render/recovery APIs. It keeps the full workflow
gate above in place; it does not qualify projection, capacity, returns, history
or complete episode execution.

Four new workflow tests cover the E7 numeric/declared-output envelope, unchanged
legacy serialization/behavior, omitted/downgraded envelopes, input/output budget
refusal, and metadata-only CLI behavior with an absent declared 3 GiB media file.
A native component test refuses a retargeted bound or unadmitted reader before
accessing deliberately missing programs. No render/provider service was called.
Native execution of v2 is still a required open feature, not completed by this
intent implementation.

Validation: all 113 workflow tests, 31 HTTP tests and 70 standard native tests
pass, including the new CLI and authority-refusal cases. Both SDK source-pin
tests (247 vendored files), all-target strict Clippy for workflow, and strict
native Clippy with/without film pass. `git diff --check` passes. These checks do
not qualify large native reads or rendering; no new effectful pool fixture was
run for this authoring/port change.

Producer-applied editor and rights/provenance checklists preserve semantic
timing, selected takes, exact source/approval subjects and separate technical
authority. A longer runtime allowance is not a change to film/cel duration or an
approval of wording, translation, voice, likeness, selection or release.
