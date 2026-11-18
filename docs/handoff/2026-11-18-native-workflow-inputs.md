# Native workflow inputs — producer handoff

Owned branch: `feat/workflow-handoff` in the isolated F worktree. Builds and
temporary evidence stay on F; no active Melena/BBJ content or producer SDK changed.

Added `reel-workflow-native`, a separate Cargo workspace and lockfile. It consumes
the 247 unchanged qualified SDK source files in `vendor/tig-workflow`, retaining
the original source-pin manifest and SDK lockfile dependency versions. Existing
REEL producers continue using `vendor/tig`. Both graphs have explicit CI checks.
The SDK source hash is f89d836d82b1a6240fbe77f4fd53b488ed57ceb303111642be53fd668ffe2cfe;
the c62c58c commit remains ancestry, not the complete dirty-source pin.

The bridge binds logical records and explicit selected/stream annex leaves in an
exact commit. Canonical input declarations are task-key-bound; machine locators
remain in a private profile. Byte-identical path substitution still requires a
new task key. Historical reads use exact bare Git metadata and never latest.
Native byte reads verify pinned executables and preserve SDK guarded handles
through the caller's operation. Public CLI output contains counts/authority
flags, not submitted paths/text. No native retention follows from a read.

Local evidence: 57 tests pass: 48 shared contracts/production tests, eight default
native bridge tests (including all 247 SDK pins), and one explicitly invoked
Windows native-read fixture. The latter used an existing synthetic pool and
previously published 1 MiB object, verifying exact bytes plus allowance and
undeclared-read refusals. No cache/store initialization, publication, hydration,
asset selection or removal occurred. Test repositories contain synthetic
metadata/pointers. The read fixture is explicit and ignored in ordinary CI.
Strict Clippy, default contract builds, both workspace format checks and diff
checks pass locally; hosted CI has not run. A missing dependency on full D
required a task-owned Cargo dependency directory on F, without changing defaults.

Scope: metadata fixtures cover exact commit, dirty/later checkout isolation,
bare Git history with unusable original files, complete leaf/stream declarations,
byte-identical aliases, tool pins and privacy-bounded CLI results. Byte reads
use the qualified single-store SDK; no adoption of the newer four-store reader,
universal storage qualification or entire native comic run is claimed.

Next bounded work: construct/admit the exact workflow resource closure through
the qualified native managed interfaces; preserve original WorkRef, activation,
source/Index checkpoints and object pins; keep durable protection while staging
and waiting; register external candidates idempotently and acknowledge retained
ownership before recording completion. Connect that implementation to the
existing handoff coordinator. Selection CAS, actor close into retained coverage
and guarded scratch retirement remain separate. No native lifecycle adapter is
installed or real project adopted by this input bridge.
