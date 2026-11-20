# V2 control close and reuse qualification

The retained v2 control from the [original issue recovery](2026-11-19-native-intent-issue.md)
is the bounded subject for guarded close and explicit same-slot reuse. Its actor
is `slot4` in `F:/src/tig-dev-temp/.tmpMzvCwc`; the final issue and both part jobs
remain separate lifetimes. No payload deletion or scratch retirement is requested.

The original frozen runtime's close preview succeeds and emits a pinned ticket:

- Evidence root: `F:/src/tig-dev-temp/reel-native-intent-close-20261119-v1`.
- Ticket SHA256:
  `49be15786b4f30cd20f33668d335e8f5b521e5fab969c3d233dfac66fe448511`.
- Close operation:
  `workflow-close-88f3f9b635988058336d438dc159f4415afa77fc7addee5f7b0e57fae7962bf5`.
- Preview reports no applied slot change, handoff or clear; it observes Index
  sequence 69. This preview alone does not qualify an applied close.

The explicit ignored fixture
`intent_close_native::retained_v2_control_close_reuses_the_slot_without_retargeting_original_history`
uses that ticket and the original runtime. It verifies a second preview against
the actual append-only Index checkpoint and identical ticket bytes, stages the
original close, injects a bounded Prepared-stage interruption, then recovers the
original handoff/clear work. It explicitly admits a new control generation on the
same closed slot, creates a protected Waiting journal, replays the old close and
checks the actual Index checkpoint is unchanged. The new admission must reopen;
the old admission must be fenced. Finally it checks the old control's retained
bytes and final issue history after reuse. Results are pending until this fixture
actually completes; compilation or preview is not close/reuse evidence.

## Applied close checkpoint and bounded continuation

The first frozen fixture completed the identical-ticket/actual-checkpoint preview
check, staged the original handoff, injected a Prepared-stage interruption and
recovered the original close. `closed.json` reports applied, original clear
complete and slot idle; replay returned the same handoff/clear WorkRefs. The
143.86-second fixture then failed while constructing the next planning packet:
it added a nonempty prompt to a deterministic tool task, which the task contract
correctly rejects. No next packet/profile/receipt/journal or reuse admission was
created. This is a fixture error after successful close, not a completed reuse
qualification. Preserve the original log and close outputs.

The corrected fixture uses the unchanged task definition with a fresh explicit
attempt and operation after completed close. The task key therefore stays equal;
activation and attempt must differ. This tests fencing even when task bytes are
identical. It is deliberate next-generation admission after a completed original
operation, not an uncertain original attempt restarted under a fresh identity.

`intent_close_native::continue_original_closed_v2_control_into_explicit_slot_reuse`
checks the existing closed outcome, activation, WorkRefs, actual checkpoint and
absence of later reuse artifacts, then continues only the reuse/history section.
It does not stage or repeat the already completed close. The original runtime and
ticket remain pinned; production Rust and SDK sources are unchanged. The explicit
new attempt remains protected and Waiting after qualification; no new producer
result is fabricated. Continuation results remain pending.

An unchanged `claims.json` genesis header is not sufficient evidence that the
append-only Index stayed unchanged. The new fixture compares the actual Index
checkpoint and separately reopens the new generation's original admission.

All close effects must retain the original operation, activation and WorkRefs.
On uncertainty, inspect/recover those same handles rather than replaying this
entire fixture under fresh identities. A new generation admitted by the fixture
will remain explicitly protected and Waiting; it is not an automatically completed
or disposable job. No actor is allocated implicitly and no production profile is
adopted. Existing S4/S5 selections and the separate large-native ImportPending
job remain outside this namespace.

Producer checklist scope follows `.roles/ROLE.md`, comics/editor and
rights/provenance. Native lifetime completion and history checks do not grant
creative approval, selection, print readiness or release authority. Final issue
actor close, larger/multilingual projects and owned runtime adoption/rollback
remain separate qualification requirements.
