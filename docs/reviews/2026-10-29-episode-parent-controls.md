# Explicit parent control providers

The episode builder can now consume a pinned parent manifest, catalog, master
template, optional source template, episode definition and season/episode bindings
through an explicit InputFiles provider. It verifies each returned metadata body
against its declared length and digest, with a 4 MiB bound per document. Provider
refusal or substitution never falls back to the local control tree. The existing
local-root APIs remain available under their existing contract.

ParentControls is a small adapter over Tig's existing ReadCache/VerifiedRead ports.
It accepts an independently admitted conform pin and configured cache places;
strictly parses its bounded metadata; derives the control references; detects
conflicting names, digest lengths, traversal and reserved manifest aliases; checks
aggregate reader/request/object/byte budgets; and holds every unique object under
effective byte/path protection through the operation. It creates no cache, daemon,
inventory, transfers or media copies. Its objects iterator exposes the exact held
top-level closure for a caller's registration/retention adapter.

```text
caller-admitted conform pin -> bounded verified manifest
                           -> derive parent control pins
configured cache places    -> deduplicated effective guards
                           -> explicit parent InputFiles
held scene inputs/outputs + bound tools + parent InputFiles
                           -> native episode checks and assembly
```

The generated full cached episode fixture now uses this port inside its existing
independent parent/child ownership checks. It stages generated metadata in fixture
caches, checks the exact unique closure, reader/byte/cooperative-mode refusals,
conflicting aliases/lengths, traversal, oversized metadata, duplicate JSON keys,
reserved names, immutable held paths, wrong expected pins and guard release after
failure. Valid local catalog bytes exist during explicit provider-refusal tests;
the builder must refuse anyway. A provider that returns the manifest bytes in place
of the catalog must fail the builder's independent metadata digest check before any
output directory is created. The successful route performs full cached native
assembly and decoded frame/PCM/timestamp checks with explicit media programs.

Editor/Sound/Rights producer review: keep selected controls exact; preserve native
scene and ordered media checks; do not confuse cache availability with source
currency, durable custody, selection or publication approval. These are producer
checklists, not independent human approvals. No actual project media/cache,
installed runtime, binding or canonical worktree changes are authorized by this
adapter or exercised by the fixtures.

Important scope boundary: this is the **parent** manifest/control closure. Native
scene job/contract/semantic controls still use their explicit input_root contract,
and imported/presentation-specific verification keeps its existing adapters. This
port does not enroll the extra control objects into a source journal by itself,
prove complete global reference coverage, authorize retiring child sources, issue
a parent producer claim or grant eviction permission. Callers must separately admit
the conform pin from their current source view and retain every owner through the
synchronous operation. Full producer integration must bind these obligations and
the remaining scene controls before claiming complete cache-only production.

L0: formatting, locked all-target/all-feature Clippy with warnings denied, diff.
L1/L2 executed: extended cached-episode fixture passes (188.61s); complete
v1/edit/v2/Git/cache fixture passes (90.77s); all 71 normal native-consumer tests
pass (174.72s), with 12 explicit external cases ignored; all thirteen episode tests
pass (32.67s); all nine conform unit tests pass (0.49s). Both full media fixtures
were explicitly executed despite their normal ignored status. Locked all-target/
all-feature Clippy with warnings denied passes (74s), optional Tig-only test
compilation passes (38.68s), and final formatting/diff checks pass. An initial
closure lifetime compile error was corrected with an explicit path-copy closure,
without relaxing any guard or validator. No local jobs remain.

Producer disposition: pass with the documented scope limits. Require exact-head
hosted Windows/Linux/FFmpeg success before squash merge and verify whole-tree
equality afterward. No actual project/cache/runtime cutover is certified.
