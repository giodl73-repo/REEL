# Streaming contained-process protocol

The contained producer now carries `reel.workflow-film-process.v2` requests for
the [v2 film intent](2026-11-19-streaming-film-intent.md). The request freezes the
streaming envelope together with the original controls, input/program locators,
runtime, workspace, limits and expected outputs. The input provider retains the
checked bound itself; request construction refuses changed process limits or
job/production control pins. Input aliases must stay in the complete declared
set. The child independently validates version/envelope agreement, unique input
file-pin totals, per-object input bounds and total output bytes. V1 requests
omit the new optional field and retain their original limits/serialization.

`film_process::render_files` returns a `Captured` candidate with file references
and private live file/namespace guards. Its caller must supply live exact native
inputs and an empty owned, capacity-admitted workspace. V2 uses the bound's
closed delivery output names. `recover_files` adopts only the exact original
successful request/outcome and rechecks the file-backed capture; missing/failed
or changed evidence refuses. It does not launch another render. Capture/adoption
is not independent media validation, retained ownership or task completion.
Callers must retain the guards and perform the normal independent checks before
publication/completion. The buffered `render`/`recover` interfaces refuse streaming
mode before effects; v1 buffered behavior remains available.

Input hashing uses the declared object allowance; output verification uses the
output allowance, so an 8 GiB master is not incorrectly limited by a smaller
input-object cap. Both use a fixed 64 KiB hash buffer, exact length/EOF checking
and short-read/interruption handling. V2 verification deadlines are checked
between reads and after EOF; the parent process deadline bounds the child,
including blocking I/O. This does not claim real-time cancellation of a blocked
individual filesystem read. Existing outcome v1 records remain unchanged and
bind the exact v1 or v2 request SHA256. Candidate evidence checks also bind the
request version and streaming envelope to the original film intent.

Three new regular component tests cover version/budget/alias rules, distinct
input/output allowances and verification deadline refusal, and changed-parameter
recovery without launch or replacement. Existing v1 component fixtures remain.
An explicit large-I/O fixture is prepared to preserve a 3,600,000,001-byte zero
file under a 4 GiB reservation using an existing F-first capacity coordinator.
It must hash the exact file under v2 bounds, refuse the v1 size allowance and
round-trip an actual retained v1 request byte-for-byte. Source/tool/result pins
and actual evidence will be recorded after execution. This fixture performs no
native admission, media rendering, provider call, publication or consumer adoption.

Full v2 workflow production and retained movie verification remain explicitly
gated by `require_native_v1`. Snapshot/projection admission, workspace/capacity
profiles, candidate/retained output budgets, bounded composition, full process
execution and interruption at real size must still be ported and qualified.
The preparatory file-backed producer port and large hash fixture do not close
S5WF-01, S5WF-02, S5WF-03 or S5WF-07.

All 113 workflow tests, 73 standard native tests, 247 SDK source pins and strict
all-target Clippy for workflow and native with/without film pass before the
explicit large-I/O qualification. No production or native-pool effect has been
performed by those component checks.

Producer-applied editor and rights/provenance checklists preserve source words,
selected takes, stable review IDs and exact approval subjects. Execution/hash
budgets grant no creative, translation, voice, likeness, selection or release
authority; all new qualification data is synthetic and private.
