# Managed native intent-bound issue qualification

## Original attempt and recovery checkpoint

The explicit fixture
`issue_native::intent_bound_issue_control_retains_and_replays_original_producers_through_native_history`
was started at `2026-11-20T02:01:29Z` against the existing declared synthetic native
pool. It creates a fresh private job namespace, admits/retains two part results,
derives and retains v2 assembly controls, then admits/retains the final v2 issue
and exercises historical reopening and editable export/restoration. The retained
intent must survive into the self-contained issue packet. This is a bounded
two-page Spanish synthetic fixture, not project or bilingual production adoption.

At the original checkpoint the process was live. Later observations found both
parts and the derived v2 control result retained. Final assembly was admitted,
with an original input ticket and Waiting journal, but ready export had not
completed. On `2026-11-20T04:22Z`, the process handle was missing and no original
fixture/runtime process remained. No terminal log result or `process-result.json`
was written. The cause/exit status is unknown; do not infer a producer failure or
a timeout from this observation.

The exact final task key is
`61b26754d745a431e59527d25b4ac0d075e612080bb3876a48005e97de919d6f`.
`issue-journal` contains the original Waiting ticket and no returned result.
`input-ticket.json` and its owned workspace remain intact. There is no
`ready.json`, worker workspace/ticket, final result or final acknowledgement.
All original claims and scratch are preserved. Full lifecycle qualification
remains open; restart of the whole fixture or re-admission is prohibited here.

The bounded recovery entry `issue_native::resume_original_intent_issue_before_ready_export`
validates that checkpoint, rebuilds the plan from frozen original parent
packets/results, checks the three retained journals and unchanged runtime, and
continues the same input ticket through ready export. It creates no new admission
or attempt and does not rewrite parent profiles. The control's original
`control-profile.json`/`control-journal` names are used explicitly. Production
Rust sources and SDK pins are unchanged from the original qualification source;
only the recovery fixture and notes are added. Recovery results are pending.

Evidence root:
`F:/src/tig-dev-temp/reel-native-intent-issue-20261119-v1`.
`tools-under-test.json` pins source/runtime/fixture before effects;
`native-v2-test.log` records the original process. `process-result.json` is
written only when that process returns. `job/original-operation-roots.json`
records the admitted namespace, including failures.

- Job root: `F:/src/tig-dev-temp/.tmpMzvCwc`.
- Source fixture: `F:/src/tig-dev-temp/.tmpjPG5Th`.
- Opening task key:
  `fc8e09b771195a662e7823a41cb6b47e4a4500051b046e4c3544e75172e4cfba`.
- Original attempt: `attempt-1`; opening actor `slot2`, operation `part-opening`.
- Opening admission Index checkpoint: genesis
  `b26f258bbabc056747780ad0817337c7885d99ec79e2465f1bf3306cb1f767e7`,
  sequence 9, hash
  `2642786bb1b2ebfda4a1d27c5aacf1328a0719017a384fbf5db1da77afec6d63`.

Keep the original profiles, Index, activation, WorkRefs, acknowledgements,
journals and scratch. On uncertainty, reconcile the original operation instead
of changing runtime pins, re-admitting or retiring its bytes. The preserved
large-native ImportPending job remains outside this namespace and is not retried.

## Exact source and tools

Source `ca9630e820cdee3ff0751410ab342559386662ab` is archived with all 1,954
tracked files individually hash/length verified. Source-manifest SHA256:
`4dd883bd6adacc7e1e0a9e7ef77c77168668b0e88efb78dade5bec7ecbe8bdf1`.

Runtime SHA256:
`b9d6dbe119f9618a3624b8b29758c66db9e5405ed166065c82374aef7eca0e10`,
39,059,456 bytes. Fixture SHA256:
`194a8abf1e1a4bea67a10f400d71bf6122d3754acc6314e44ffc9c19dd8ffa30`,
44,352,000 bytes. Both execute from immutable hash-named tool directories. The
fixture's explicit runtime override must match its built runtime's exact bytes;
the new job profiles point at that frozen runtime from first admission.

The native input descriptor is the existing
`F:/src/tig-dev-temp/reel-workflow-native-20261118-v1/reader-fixture.json`, SHA256
`d3b14db2732440ddc81a3fd05b0b9d8d690bb2af53654e9b4f65451405e9dec3`.
The vendored qualified Tig SDK and native limits are unchanged. Native fixture
compilation and strict all-target Clippy with film pass; these do not substitute
for the in-progress lifecycle result.

Producer checklist scope follows `.roles/ROLE.md`, comics/editor and
rights/provenance: actual ordered parts and source identity remain separate from
canon, translation, likeness/voice, font rights, creative approval, selection,
print readiness and release. No independent review or project approval is claimed.
