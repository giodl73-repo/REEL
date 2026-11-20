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
with film passes. A separate ignored fixture is provided to replay the previously
exported synthetic two-part issue through generated v2 controls; its actual result
is recorded below when run. This is not a new native admission fixture.

Producer checklist findings (comics editor, story/editor, platform/audience and
rights/provenance under `.roles/ROLE.md`): authored part/page order remains
explicit; language is checked against the publication rather than inferred;
the captured editable issue retains its intent; and byte verification cannot
grant canon, translation, font/voice rights, approval, print readiness or release.
These are producer-applied technical checks, not independent review or approval.

Managed v2 admission/retention, post-close native history, larger project comics,
film/conform order binding and full bilingual S5 production/adoption remain open.
The preserved large-native ImportPending job and all existing profiles/claims
remain outside this change.
