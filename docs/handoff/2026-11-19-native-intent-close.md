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
