# V2 control close and reuse qualification

Guarded close recovery and explicit same-slot reuse now pass for the retained v2
control. A new protected attempt uses the same task definition with a distinct
activation. Old-close replay leaves the actual Index checkpoint unchanged, the
old admission is fenced, and both old control and final issue native history
verify after reuse. The new attempt remains Waiting and protected.

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
bytes and final issue history after reuse. Evidence below combines the original
applied close with its explicit continuation; the failed first fixture is not
reported as a successful uninterrupted run.

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
result is fabricated. The frozen continuation passed in 218.14 seconds, exit 0.

## Verified result and pins

The old and new task key both equal
`4199e26b4bd5cb4926b0bd076f43e1046cd14b2019f01ad35f11985b02b4d932`.
The original attempt is `attempt-1`; the deliberately admitted new attempt is
`reuse-attempt-1`, with operation `intent-control-after-guarded-close`.
Old activation SHA256:
`05a1c9315c4f7e4d482feb04771f7b98c4cdcb713d076e4aa2e6a069faac3efb`.
New activation SHA256:
`b79d660a2e8a6e9df16eb71d2e7434b0af2a922ccd16140e37f967f379812d4a`.

`verified-preview.json` binds an unchanged actual checkpoint and identical close
ticket. `staged.json`, `closed.json` and `closed-replayed.json` retain the original
handoff/clear WorkRefs. `close-after-reuse.json` reports original clear complete
and slot not idle; the actual Index checkpoint stays equal across old replay and
post-reuse issue readback. The new admission reopens, while the old admission
refuses with a stale/conflict/denied fence. Native verification reopens the old
control result, and `post-reuse-issue-history.json` reports the original final
issue Retained and completed, unselected, with scratch retirement unauthorized.

Continuation source `07e388fe14595346bbde4034371d45057db178ab` is archived with
all 1,957 tracked files individually hash/length verified. Source-manifest SHA256:
`96c818c06ec2923ba26dd84ac6463e306bb59c5e5d20833b2fc739bcccf25a1b`.
Frozen fixture SHA256:
`6318e925791d5520c55e07e0eddb8cdb64f95ac6f8be67e0e8b6ae9cb7b1ac2d`,
44,537,344 bytes. The runtime remains the original frozen
`b9d6dbe119f9618a3624b8b29758c66db9e5405ed166065c82374aef7eca0e10`,
39,059,456 bytes. Both initial and continuation source/tool pins precede their
effects; production Rust and SDK source remain unchanged.

`qualified-lifecycle.json` pins 25 control/log/result files, preserves the first
fixture's planning failure and the successful continuation, and records the new
protected admission. Certificate SHA256:
`088f8e5228188eff2b3d783bd41e27b2d38476808ba4ef2cf3dece3ab7a00e4e`.
All evidence is under the private root above; no payload or scratch is removed.

All 85 standard native tests pass (72 ignored fixtures excluded), with the
explicit 218.14-second continuation passing separately. Strict all-target Clippy
with/without film, all 247 SDK source hashes/lengths and `git diff --check` pass.
`qualified-validation.json` pins the lifecycle certificate and raw regression log.
Validation certificate SHA256:
`69dfa023825b1a09a6d1f4f43fa19d1ea0795c6289bd0e4c3b8efe2bfc794eaa`.
Continue the new Waiting lifetime through `next-journal`,
`next-handoff-profile.json` and `next-receipt.json` under the close evidence root.
Preserve its `reuse-attempt-1` identity and new activation; neither qualification
entry may be restarted against these already advanced checkpoints.

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
