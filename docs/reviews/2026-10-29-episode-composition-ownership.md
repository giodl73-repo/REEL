# Assembly and child source ownership

The scene-only native composition adapter now derives all selected component pins
and original checked-claim pins from the admitted source and opaque current-scene
proofs. It does not accept a second authored component or receipt inventory. The
existing native job metadata format retains these as Composition and Evidence;
compiled scene movies never enter Primary, Runtime or ClaimedOutput as a shortcut.
Hashed segment labels avoid path aliases and prefix collisions.

A small immutable Evidence declaration retains the exact expected child checker
recipe fingerprints. Changing a child checker contract changes the durable parent
job even when its source checkpoint, retained movie and original claim are the
same. Replay cannot silently exchange these requirements. This declaration is
derived metadata, not another user-maintained asset inventory. Independent child
obligations remain necessary; this is not permission to retire their source roots.

The parent uses its own frozen assembly context, consumed recipe and actually bound
runtime. Finish/reopen readback verifies the exact runtime manifest and dependency
resources. A parent source is adopted only with exact current independently held
child jobs, distinct parent/child source identities, and a bounded unique resource
union. The union includes the child primary closure, controls, interpretation,
runtime, selected movies and original claim obligations. Logical declared bytes
are not allocated disk usage or a capacity/availability certificate.

```text
captured source -> assembly context / current recipe / held runtime
               -> exact scene proofs -> selected movies + original claims
               -> finish / reopen parent job metadata
held child jobs + exact parent readback -> independent parent source adoption
               -> check parent AND children
               -> bounded action with caller-held media handles
               -> check parent AND children, including on action failure
```

Only individual source identities are fenced. Independent workers remain possible.
Drop releases the fence, never withdraws durable roots. Exact revision/owner/
selection/journal checks apply on resume; historical operation replay cannot mint
current authority after withdrawal. Late adoption/postcondition failure retains
already adopted obligations for explicit reconciliation. A callback may have side
effects before a failing postcondition; withholding its result does not roll those
effects back or grant permission to delete them.

Producer review uses Editor, Sound Designer and Rights/Provenance checklists.
Editor requires exact ordered components and no hidden duplicate inventory. Sound
requires native decoded-media validation to remain separate from hash membership.
Rights/Provenance requires original claims to remain original, actual runtime
identity, durable obligations on failure, private local evidence, and no technical
success represented as selection or delivery approval. These are producer review
lenses, not independent human approvals.

L0: locked all-target/all-feature Clippy with warnings denied, formatting and diff.
L1/L2: extend the generated cache-only episode fixture with exact Composition and
Evidence sets, manifest/selection substitution refusal, resource-union bounds,
parent/child source collision refusal, per-source fence isolation, read-only exact
resume, Drop persistence, historical retry refusal, and parent/child changes during
the action. The successful path performs actual native assembly using independently
checked cached scene media while both ownership layers remain held.
The cached assembly action now injects the already bound runtime explicitly into
both scene validation and the episode builder. A different checker dependency
fixture preserves source/movie/original-claim identity but changes one child's
ownership recipe; durable replay and parent adoption against the old child guards
must refuse it before journal mutation. No rerender is used for that negative case.

Scope: this is parent/child ownership orchestration. It is not a complete episode
producer or a parent production claim. Presentation components deliberately refuse
until their own eligibility proof is implemented. Generated-primary eligibility,
parent checked publication/retention, safe child-source retirement, full-family
migration, actual custody, LFS/hooks and reversible project cutover remain separate.
No actual project media/cache, daemon, installed runtime or binding changes occur.

Local execution so far: final locked all-target/all-feature Clippy passes (55.49s),
the extended cached episode fixture passes (152.35s), all thirteen standalone episode
tests pass (28.10s), and the complete v1/edit/v2/Git/cache lifecycle passes (80.03s).
The first normal consumer run reported seventy passes and one existing recipe test
failure at Source::open with the generic Git error, not a semantic assertion. That
exact test passes alone without code changes (23.51s). Its cause remains undiagnosed;
the full unchanged consumer suite is being rerun, with no retry/timeout/validation
relaxation in production code. An initial command selected zero tests using the
wrong target filter; the actual thirteen-test target was subsequently run explicitly.

The unchanged full consumer repeat passes: 71 passed, 12 explicit external cases
ignored, no failures (162.74s). The two full media fixtures above were explicitly
executed despite their normal ignored status. Optional Tig-only test compilation
passes (38.15s), as do final formatting and diff checks. No local jobs remain.

Producer disposition: pass with the documented scope/diagnostic risks. Exact-head
hosted Windows/Linux/FFmpeg qualification remains required before merge.
