# Workflow source entry

The film-first workflow crates are available on REEL main. Start with their
documented commands and the [PR plan](workflow-pr-plan.md) for remaining gates.
This page describes source availability; it does not install tools or adopt a
producing project's profile.

| Task | Entry point | Current boundary |
| --- | --- | --- |
| Export prompts and accept returned files | [`reel-workflow`](../crates/reel-workflow/README.md) | Manual packet/result contracts and byte checks; no native retention or task completion |
| Inspect existing bilingual scene metadata | [Film inspection](workflow-film-inspection.md) | Stable story/cel/cue identities, selected versions, exact approval subjects and language-local clocks; no media rendering or creative approval |
| Protect manual work and retain external results | [`reel-workflow-native`](../crates/reel-workflow-native/README.md) | Explicit native admission, owned staging, immutable unselected ingress, retained readback and original-operation recovery/close |
| Run an existing film producer | [Owned film CLI](../crates/reel-workflow-native/README.md#owned-film-cli-optional-windows-film-feature) | Optional Windows `film` feature: pinned prepare/run/resume/verify/close, existing media reuse, capacity and scratch preserved |
| Call an API or workflow proxy | [`reel-workflow-http`](../crates/reel-workflow-http/README.md) | Pinned JSON POSTs, private original-operation evidence and offline resume; proxy result/returns files feed explicit native handoff/retention |
| View cels, versions and language cues | [`reel-workflow-ui`](../crates/reel-workflow-ui/README.md) | Existing-film scene projection, read-only pinned local board and explicit journal observations; desktop and narrow playback verified with synthetic media. Native previews and episode adoption remain open. |
| Put stable cel IDs in a scene review movie | [Private cel-ID copies](film-cel-review-v1.md) | Existing delivery-receipt frame ranges, visible IDs and exact source map; candidate output with native retention and issue/fix dispatch separate |
| Turn film findings into issue drafts | [Exact-baseline findings](workflow-film-findings.md) | Checked movie/map/receipt bytes and exact target frames; private drafts/API bodies with posting, repair and native retention separate |
| Submit an explicitly authorized finding | [GitHub issue adapter](workflow-github-issues.md) | Existing owned HTTP operation, exact created-response receipt and offline replay; uncertain remote outcomes, repair and selection remain separate |
| Prepare an image-candidate repair | [Film image repair](workflow-film-image-repair.md) | Exact report/job/source-map checks against the declared baseline, ordinary image task and consumer/language impact; approved application and rebuild execution remain separate |
| Prepare affected scene renders after a still repair | [Native film rebuild](workflow-native-film-rebuild.md) | Per-language packet, source and parent files for protected admission. Scene control verification preserves the original recording and clock. |
| Assemble an episode from checked components | [Bounded episode jobs](tig-episode-jobs.md) | Existing `reel-tig-job` producer for ordered scenes and presentations, with component verification and retained handoff recovery. |
| Record an authorized speech candidate | [Speech generation](tig-speech-generation.md) | `reel-tig-speech` writes a candidate recording, alignment and receipt under the pinned request and authorization. |

Use each crate's `capabilities` and `--help` commands to check the binary you
actually built. The native and HTTP crates have separate manifests/lockfiles;
the film producer path requires the native crate's optional `film` feature.
Their READMEs contain exact build/check commands, request fields and limits.

For a provider result, accept native protection and stage any required inputs
first; perform the explicit HTTP/proxy operation; then import and verify its
candidate through the original native handoff. Reconcile uncertainty using the
original task/attempt, request, registry, workspace, runtime pins and native work.
Never allocate a new attempt merely because a response was lost. The synthetic
CLI bridge qualifies this composition through retained verification and slot
close. It uses no real account, project media or new voice performance.

Close is separate from scratch retirement, capacity release and selected-asset
changes. Keep private prompts, machine bindings, source/media files and evidence
in project-owned storage. Technical validation grants no creative approval,
voice authority, candidate selection or publication permission.

## Remaining production gates

The [visual production workspace requirements](workflow-ux-requirements.md)
describe the near-term cel/cue/page board, existing editor reuse and generic
git-asset tool/preview handoff gaps. This is proposed UX work, not an existing
complete live production dashboard. The first local board implementation is
linked above; native previews and live project integration remain open.

The [protected repair qualification](reviews/workflow-protected-film-repair-review.md)
verified image and control retention through one synthetic scene repair.
Automatic dispatch across affected languages and episode consumers remains open.
Provider orchestration must connect speech candidates to workflow retention and
review. Existing approved recordings are reusable as exact cue inputs.

W5 still requires packaged/pinned entry tools, project acceptance, owned-profile
adoption and rollback. Where episode-scale native composition is required, W6
must qualify the actual resource closure, large masters, interrupted execution and
historical outputs first. The current native envelope/response limits do not
cover the declared Season 5 production scale. Use the existing bounded episode
producer with the project's accepted profiles for current production. Qualify
workflow integration against the actual ordered components and retained outputs
before adopting a replacement profile.

Comics are a separate publication lane and are not a film release gate. Current
Season 5 production, selected media, recordings, approvals and active profiles
remain with their existing accepted workflows until explicit project adoption.
