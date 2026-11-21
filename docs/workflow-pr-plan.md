# Film-first workflow landing plan

Status: reviewed implementation plan, not a production-readiness declaration.
Source landing: [W1 #386](https://github.com/giodl73-repo/REEL/pull/386),
[W2 #388](https://github.com/giodl73-repo/REEL/pull/388), and
[W3a SDK intake #389](https://github.com/giodl73-repo/REEL/pull/389) are merged.
The native lifecycle slice adds protected manual work and retained close/recovery.
W3a's producer execution and episode-scale gates remain open; no source merge
adopts an actual project or production profile.
The [role review](reviews/workflow-pr-plan-review.md) records findings and their
disposition. Roles are author-applied checklists, not independent reviewers.

The first release adds workflow orchestration around existing accepted film
producers. Existing drawings, recordings, timing receipts and approvals are its
inputs. A producing project keeps its active tools and profile until an explicit
owned-profile adoption and rollback check passes.

## Pull request sequence

This sequence defines acceptance beyond the source landings above. Each implementation PR
must identify its dependencies, exact supported behavior, validation and limits.
Extract changes from the integration branch onto current main; do not submit the
entire integration branch or infer merge readiness from its aggregate test count.

| ID | Pull request | Dependencies | Required acceptance |
| --- | --- | --- | --- |
| W1 | Workflow contracts and manual prompt/result loop | None | A film task exports runnable prompts, accepts exact returned candidates, detects stale/cross-attempt results and resumes without duplicating an effect. Default build and tests require no comic renderer or provider account. |
| W2 | Existing-film project adapter | W1 | An existing project is inspected into a sidecar with exact asset/take versions, stable story/cel/cue IDs, approval subjects and language-local clocks. Inspection calls no model, renders no media and changes no active profile. |
| W3a | Qualified storage lifecycle and existing film producers | W1, W2 | Separately review SDK/source pins and adapter changes. An owned film step invokes pinned accepted tools, protects declared inputs, retains/reopens outputs and recovers a completed render without repeating it. Admission previews include input, output, registration and close bounds before production. |
| W3b | Direct API/proxy execution and speech adapters | W1, W2 | Manual and direct modes have the same task/result contract. Loopback transport tests cover success, refusal and uncertain outcomes. Reuse existing speech integrations, pin service/model/version and require operation-specific voice authority for new takes. Existing selected recordings remain reusable inputs. |
| W4 | Film review, issue/fix and affected rebuilds | W2, W3a | Review copies expose stable cel targets. A controlled technical finding produces a reviewable issue draft and a repair; only affected languages/consumers rebuild. Exact approvals stay bound to their subjects, with no inferred creative approval. |
| W5 | Film release, owned adoption and handoff | W1–W4 | One documented entry point, pinned binaries/source, a project example and a capability matrix. The acceptance project runs in an isolated owned profile; adoption and rollback preserve current/prior outputs. Unsupported capabilities refuse before effects. |
| W6 | Additional native episode composition | W3a; precedes W5 wherever required | Qualify complete declared resource closure, large masters, bounded component decomposition, long execution, interruption and historical readback. It is a follow-up only where W5's accepted producer path already meets its advertised production needs. |
| C7 | Comic publication workflow | Shared contracts after W1; independent of film landing | Separate comic assembly/review/repair/source-restoration checks and publication-specific readiness. Existing comic work remains preserved; comic qualification is not a film release gate. |

W3a's SDK pin/intake change and lifecycle/producer integration must be separate
stacked PRs if the SDK diff would obscure the adapter review. Exact qualified
source manifests remain authoritative; an ancestry commit alone is not a pin for
qualified working-tree source. Existing production SDKs must not be upgraded
implicitly.

## Film acceptance contract

W2 defines these choices; W3 and W4 implement or explicitly reject them according
to the selected producer's capabilities:

- Preserve source canon, scene order, separate narrator/poet roles, cast and
  continuity references, exact text versions and selected performance authority.
- Declare pacing, desired cel duration/density, animation school/style and
  renderer choice. Existing HTML/CSS/SVG producers have explicit pinned render
  settings; an authoring field is not proof that a renderer supports it.
- Keep semantic trigger identity separate from each language's trigger words,
  selected-take sample positions and duration. Never reuse one language's clock
  or silently rewrite source text to make timing fit.
- Preserve complete selected opening/ending material, poem/title/attribution
  breaths and lyric synchronization. Credits derive from actual selected uses.
- Derive captions from actual conform/selected-take receipts. Check complete
  order, frame/sample clocks and the required review and portable editions.
  Record actual viewing/listening separately from automated media checks.

The first release's capability matrix distinguishes existing-producer
orchestration, bounded native execution and full native episode execution.
An unfinished replacement renderer must not prevent use of an already accepted
producer. Equally, a tiny fixture or metadata-only scale declaration must not be
presented as full episode qualification. If the existing-producer path cannot
meet the acceptance project's scale or delivery needs, the necessary W6 work is
required before claiming that production capability in W5.

## Review and release rules

1. Each PR is independently buildable, or explicitly stacked on a named PR.
   Show its focused tests and actual dependency changes. Film-only checks remain
   available without comic features. Do not mix private project fixtures or
   unrelated producer changes into the code PR.
2. Read-only inspection performs no provider calls, hydration, rendering, upload
   or automatic deletion. An uncertain operation reconciles its original
   identity, work references and output evidence before another effect.
3. Public fixtures are synthetic. Project-specific acceptance evidence stays in
   project-owned storage. Public summaries omit private assets, prompts, voices,
   work identities, machine paths, credentials and private hashes.
4. Provider transport fixtures are not real service/account qualification.
   Capabilities report that distinction. Exact approvals, candidate selection,
   private delivery and public release remain separate decisions.
5. A source PR may merge after its scoped checks and review even while later
   production gates remain open. Software merge does not adopt a production
   profile, select a creative candidate or authorize a film release.

## First execution steps

Land this reviewed contract, then extract W1 as the first implementation PR.
Audit the current-main dependency graph before moving code: publication types,
assembly helpers, optional comic features and isolated SDKs must have explicit
owners and dependencies. Preserve the integration branch and unfinished job
identities as checkpoints. Do not start new comic production or replace an
admitted job's runtime as part of this film-first landing.
