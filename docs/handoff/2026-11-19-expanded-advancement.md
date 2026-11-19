# Durable expanded-task advancement

```text
reel-workflow-native advance-expanded PLAN FROZEN FROZEN_SHA256 CONFIG PRIVATE_DIRECTORY --apply
```

This joins frozen recipe expansion to actual native admission and graph adoption.
The explicit configuration has schema `reel.workflow-advance-config.v1`, a pinned
current lifecycle `runtime` and an enrolled retained `target`. It does not select
an actor automatically; the frozen admission already names the original actor,
operation and attempt. The target's enrollment, owner, store coverage and ready
state are checked before a new admission.

The private directory records an immutable intent bound to its actual directory
identity, base plan, frozen expansion and configuration. Native/source/parent
control namespaces are refused. A scoped writer fence serializes advancement;
there is no global renderer lock. Controls use exclusive creation, flush and
readback. Existing bytes must match exactly; partial controls and foreign
directories are preserved and refused.

Advancement recovers a completed original admission when it already exists.
Only a typed missing-original-work result permits first admission, using the same
frozen operation. Pending/uncertain work is preserved for explicit recovery.
If the standalone receipt was lost, the original durable profile supplies its
exact witness; otherwise native recovery supplies the original WorkRef and
activation. A newer checkpoint must not rewrite an existing profile.

The capsule contains `intent.json`, `frozen.json`, `config.json`, `admission.json`,
`profile.json`, an acknowledgement directory, the append-only handoff `journal`,
and complete `plan.json`/`bindings.json` outputs. The new task is Waiting and its
binding pins the exact profile bytes. An existing Waiting or Retained journal
must match and verify; advancement does not resume import effects or cancelled
tasks. Other original run bindings are preserved.

Scratch staging remains an explicit subsequent boundary: prepare and pin the
original staging ticket, add it to a new run-binding snapshot, then export the
protected prompt batch. Deterministic worker dispatch and returned-response
ingress use the existing graph commands. No provider is invoked by advancement.
This is one-task advancement from a frozen proposal, not an automatic multi-wave
runner, capacity allocator, uncertain-acceptance repair or project adoption.

Three metadata/claim fixtures cover first adoption, unchanged-index replay,
original-admission reconciliation, lost-receipt recovery, graph visibility,
invalid targets, foreign directories, changed configuration and partial-control
preservation. A separate opt-in existing-pool fixture covers the CLI path,
replay, actual native staging and protected batch export; qualification is pending.

Producer-applied recovery/custody and rights/provenance checklists preserve
original identities, custody boundaries and private operational controls.
These checks grant no independent review, creative approval or public release.
