# Derived intent-bound comic controls

`reel.workflow-issue-control.v2` adds a required `composition` specification to
the existing derived issue-control request. It pins the independent intent and
part-to-producer map. The request names receipt/carrier paths before their bytes
exist; generation fills in their exact hashes after the parents have returned.
It emits `reel.comic-issue-assembly.v2`, preserving that same pinned intent.
V1 without composition remains byte-identical; mixed version envelopes refuse.

The generator checks the actual issue plan's edition, language and ordered
producer mapping, reads the declared intent as a bounded exact input, and checks
its project. Its own task must be listed among the edition's final control
parents. It must consume every component producer and may consume other declared
control parents, excluding itself. Hidden parents and absent components refuse.
The final assembler independently requires the exact full component/control
parent set, including the generator's returned request.

Generated metadata includes expected original receipt/carrier selectors. Native
generation, return checking and historical replay validate them against frozen
original-parent evidence under live admitted input authority, before accepting
the derived controls. Native custody, candidate retention, selection and task
completion remain separate gates. No LLM transport is involved in this tool task.

Three added contract tests cover v2 generation followed by final assembly binding,
fixed requests across unknown future drawing bytes, and refusal of wrong
project/language/control/producer, stale intent pins, hidden or missing parents,
missing intent source bindings and mixed versions. The 131 workflow tests pass,
including the legacy exact-byte generation test. Strict workflow/native Clippy
with film passes. A separate ignored fixture replays the previously exported
synthetic two-part issue through generated v2 controls. This is not a new native
admission fixture.

## Actual local replay

The frozen fixture passed in 2.11 seconds. It reads the exact existing two-part
editable export from the prior qualified derived-control fixture, verifies every
supplied source descriptor, derives a v2 assembly request and deterministically
replays its two actual rendered pages. Its PDF and transcript descriptors equal
the original issue receipt, and a second replay from the captured editable packet
produces those same descriptors. The captured packet includes the pinned intent.
Swapped opening/closing producer mapping refuses.

This qualifies generation-to-render correspondence on local exported synthetic
bytes. It does not reopen those bytes through the native reader, acquire claims,
admit a new native v2 task, acknowledge retained ownership or complete a native
job. The report explicitly marks native byte access/lifecycle, completion and
selection false. Existing native profiles are neither changed nor adopted.

Source `36f8257710174fff8334b4b3a582413a68f2101c` is archived with all 1,954
tracked files individually hash/length verified. Source-manifest SHA256:
`b2afd79b5b8172d80f464bc3d10f339b1b6284897c77b9684bd8386bd0d06809`.
Frozen fixture SHA256:
`92c053f27be2ad9a841c1d8a1a8eca0182cbcff934134d5d1e8f07c50b2c74fb`,
11,323,904 bytes. Evidence root:
`F:/src/tig-dev-temp/reel-derived-intent-controls-20261119-v1`.
Its `tools-under-test.json` pins the source and executable before replay;
`replay/report.json`, `intent.json`, `request.json`, `packet.json`, `assembly.json`
and `receipt.json` retain the exact private controls and result descriptors.
The original exported packets are read in place, without a copy or new render
of selected project media. All comic bounds and Tig SDK pins remain unchanged.

Producer checklist findings (comics editor, story/editor, platform/audience and
rights/provenance under `.roles/ROLE.md`): authored part/page order remains
explicit; language is checked against the publication rather than inferred;
the captured editable issue retains its intent; and byte verification cannot
grant canon, translation, font/voice rights, approval, print readiness or release.
These are producer-applied technical checks, not independent review or approval.

All 131 workflow and 85 standard native tests pass; the native suite excludes 68
ignored fixtures. The explicit frozen local replay above passes separately.
Strict all-target Clippy passes for workflow and native with/without film, all
247 SDK source hashes/lengths match, and `git diff --check` passes. Raw logs and
control hashes are pinned in the evidence root's `qualified-tools.json`, SHA256
`b2db44f56cfd60299b089f468baf02698bbda0cd2083e52269f6950e7a8701e1`.
Its scope is local generated-control/render correspondence, not a lifecycle-
qualified native distribution.

The [managed v2 issue recovery](2026-11-19-native-intent-issue.md) separately
qualifies bounded native admission/retention and bare-history editable replay for
two Spanish synthetic pages. It recovers the original interrupted final task,
without a new admission/attempt or runtime adoption. Guarded close/reuse,
post-close history, larger/multilingual project comics, film/conform order binding
and full bilingual S5 production/adoption remain open.
The preserved large-native ImportPending job and all existing profiles/claims
remain outside this change.
