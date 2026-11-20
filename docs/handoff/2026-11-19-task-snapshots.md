# Exact task snapshots across plan revisions

`reel.workflow-plan.v3` adds a complete `task_snapshots` map from authored task
ID to exact Git commit. Every task must appear once; unknown/missing entries,
zero commits and malformed identities refuse. The plan-wide `source_commit`
identifies the authored overall revision. It is never a fallback for an omitted
task snapshot. `make_packet` uses the task's snapshot, preserving the existing
packet v2 identity algorithm and native record/annex checks.

Legacy plan v2 serialization omits the empty map and retains its original
plan-wide behavior. A nonempty map in v2 refuses. Existing revision v1 repair
candidates remain byte-compatible v2 plans; they still require declarations for
every native task. They cannot silently flatten an already scoped v3 plan.

`reel.workflow-repair-revision.v2` produces a v3 candidate. Only the explicit
repair impact's rebuild closure advances to the revision commit, and exactly
those native tasks require new commit-bound declarations. Other tasks retain
their original snapshots, declarations and packets, provided their exact
dependency results remain unchanged. Neither mode carries results into a
changed packet, grants custody or applies a selection. A scoped baseline's
review must link the exact publication and language to a single unambiguous
task snapshot; absent, mixed or stale context refuses.

[Rooted repair v2](2026-11-19-rooted-part-repair.md) now explicitly selects changed
branches when parts share master/plan records. Unselected parts retain their
historical references as well as snapshots; every selected branch's dependent
consumer still rebuilds. Global repair v1 retains its original behavior.

`reel.workflow-upstream-inputs.v2` permits a child to consume exact parent results
from different snapshots. The parent packet/result key, project, declared task,
canonical result SHA, artifact identity and explicit selector remain checked.
Identical bytes under another parent snapshot do not satisfy that result pin.
Version 1 still requires equal parent/child commits. Scoped expansion generates
v2 upstream controls and pins new tasks to the overall revision; append-only
recipe and native frozen-expansion checks preserve every original task snapshot.

Six new protocol tests exercise legacy JSON, complete-map refusal, unchanged
packet equality, stale affected-job refusal, exact cross-snapshot dependencies,
repair native-declaration scope, review context, and recipe/expansion snapshot
tampering. These are pure planning/observer checks. A successful observer is
synthetic evidence, not native retained ownership.

The native frozen-expansion component test also refuses retargeting either a base
task or a new task before admission, preserving metadata and empty source claims.
All 109 workflow tests and 31 HTTP tests pass, together with the existing 65-test
native regression suite and the new targeted native test (66 total). All-target
strict Clippy passes for workflow and native with/without film; `git diff --check`
passes. These native component fixtures use bounded synthetic metadata, separate
from the effectful completed-render qualification at its frozen earlier source.

The [actual A -> B native history fixture](2026-11-19-native-task-snapshots.md)
now passes retained native graph observations, exact reads from the new bare
commit, unchanged task/consumer packets, stale retargeting refusal and restored
baseline packets, with identical claims, journal and review copy/IDs. Effectful
owned project/profile adoption and rollback remain open.
S5WF-03 also still needs immutable drawing/story identity and exact
approval/consumer integration; this change addresses plan-wide key churn only.
No S4/S5 production, selected take, approval, current selection or existing job is
changed. All v1 resource and execution bounds remain.

Producer-applied editor and rights/provenance checklists find that exact source
lineage and affected-consumer isolation are preserved by the explicit contracts;
actual pacing, translation, voice consent and release remain project decisions.
This is a technical checklist application, not an independent review or creative
approval.
