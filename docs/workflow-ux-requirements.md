# Visual production workspace and git-asset handoff

Proposed product/integration requirements, not an implemented UX or an upstream
contract. The user needs to see cels, cues and other assets assembling into a
show or comic while production runs. Terminal output remains diagnostics rather
than the main production experience. No server, asset scan or project operation
was started for this audit.

## Existing surfaces and confirmed gaps

Audited public git-asset main at
[`f12a163`](https://github.com/giodl73-repo/git-asset/tree/f12a1634a8833f822edf2a78b4df75a2b3851538).
Its [catalogue contract](https://github.com/giodl73-repo/git-asset/blob/f12a1634a8833f822edf2a78b4df75a2b3851538/specs/CATALOGUE.md)
already supports project/work navigation, registered versions, related records,
comparison and explained impact. Work collections expose configured members,
stages and facets. Its
[enrollment structure](https://github.com/giodl73-repo/git-asset/blob/f12a1634a8833f822edf2a78b4df75a2b3851538/crates/git-asset/src/catalogue/mod.rs)
contains display, fields, graph profiles, collections and storage, with no general
tool/action registry. The audited catalogue HTTP routes expose metadata queries
and explicit storage review, not a production-job event or media-playback API.

The current
[UI roadmap](https://github.com/giodl73-repo/git-asset/blob/f12a1634a8833f822edf2a78b4df75a2b3851538/docs/CATALOGUE-UI-PLAN.md)
explicitly leaves **U4: fenced handoff to enrolled authoring tools** open, along
with previews/playback. This is an integration gap, not proof that adding a REEL
display field registers an executable UX.

REEL already has a [cel sorter](cel-sorter-v1.md) with thumbnails, cue navigation,
selected-recording playback and scoped language editing, plus note, underlay,
screenplay and voice-review surfaces. The
[shared editor host](editor-host-v1.md) provides private loopback hosting,
capability URLs and byte-range media access. Its current session is a frozen
export with a 1 GiB combined media bound; it has no live workflow/job endpoints.
It should be reused where applicable, not described as an episode-scale live
production board. The declared large-master/resource scale needs a bounded,
lazy preview path rather than copying a whole episode into that export.

## REEL experience

The primary screen should be a project/work production board:

- Film: scene-grouped cel cards with actual images and stable story/C IDs;
  language-local cue playback/transcripts, selected takes, music/SFX layers and
  an assembled review preview when available.
- Comic: issue/page/panel navigation with composed page previews and explicit
  artwork/lettering/language views, sharing the same candidate/version core.
- Creation: show queued, generating, waiting for manual return, verifying,
  publishing/retaining, review and failed/uncertain states beside the asset.
  Show a preview when exact bytes are actually ready; do not invent progress.
- Review: compare candidate versions, see prior selected/approved subjects,
  comment on a stable cel/cue/panel target and navigate to its review clock.
- Recovery: explain what is waiting and resume the original task/attempt. A
  button must not silently replace an uncertain API call or release its claims.
  Closing or reconnecting a UI session must not cancel the job or regenerate assets.

Existing S5 drawings and recordings must appear as reusable inputs. Spanish and
English share semantic identities while keeping their own cue words, takes and
sample/frame clocks. Generated, retained, reviewed, approved and selected are
distinct visible states. Logs are an optional detail panel.

## Generic requests for git-asset

These are requirements to discuss upstream; the names below are descriptive,
not existing API/schema names.

| Request | Required behavior | REEL responsibility |
| --- | --- | --- |
| Tool/view registration | Enroll a versioned tool identity, supported asset/work capabilities and trusted local launch/connection mechanism. Offer explicit actions such as Open in REEL; never execute arbitrary record-authored commands. | Supply the production-board/editor application and supported capabilities. |
| Exact launch context | Hand off project/work, record or candidate identity, immutable snapshot/version/stream, variant context and current basis. Preserve return navigation and historical identity; reject stale context. | Interpret language, cel/cue/page identity and authoring semantics. |
| Guarded preview access | Serve explicitly named image/audio/video preview objects with MIME/length/hash, bounded reads/ranges, declared availability and source fencing. Missing/offline bytes stay explicit; opening a gallery does not hydrate or render. | Produce and register any needed thumbnail, waveform, proxy movie or composed-page derivatives through normal jobs. |
| Candidate discovery | Expose unselected candidate identity and availability separately from current selected records, including safe exact handles where candidates are still protected by active work. No selection side effect. | Associate candidates with scene/cel/cue/page and review/version history. |
| Observation and navigation | Provide bounded change/cursor or invalidation semantics for registered assets/claims, plus stable links back to asset history/impact. Keep incomplete coverage explicit. | Publish/observe workflow job progress and translate it into production states; git-asset need not understand scene-making. |
| Action/result fencing | Preserve source/basis, original operation and returned candidate identity across tool sessions. Return draft/candidate results without implying approval or selection. | Validate edits, run/recover jobs and require explicit selection/adoption operations. |

The initial handoff can be a trusted tool launch or capability-scoped local URL;
it does not require loading arbitrary third-party JavaScript into the catalogue.
Physical cache paths and credentials remain provider-owned. A preview link is
not custody, creative approval or authority to remove bytes.

## Recommended delivery sequence

1. **Visual board first:** a read-only REEL board over existing exact assets and
   workflow journals, showing actual cel/cue/page previews and live task state.
   Launch it explicitly. Reuse existing components and native readers; no new
   asset database/cache engine. This can ship standalone while U4 is developed.
2. **Catalogue handoff:** qualify generic tool registration, exact deep links and
   return navigation with git-asset. Use the same REEL board, rather than a second
   catalogue or a REEL-specific core inside git-asset.
3. **Review and fixes:** connect stable-target annotations, candidate comparison,
   existing editors and the W4 issue/fix/affected-rebuild flow through the same
   backend contracts as the CLI.

First acceptance should show one synthetic bilingual scene's existing cels and
cues, then one returned image candidate appearing under its original attempt
without disturbing an older selection. Cover unavailable previews, reconnect,
stale events, exact historical navigation and preserved uncertain jobs. Include
a synthetic comic page projection without making comic completion a film gate.
The first UI PR must include actual synthetic browser review at desktop and
narrow sizes with visible media and cue controls; DOM/controller tests alone do
not establish visual or playback acceptance. Real-project adoption and scale
remain separate qualifications.

Producer-applied [.roles](../.roles/ROLE.md) lenses: editor requires visible,
stable targets and correct language clocks; platform/audience requires useful
previews and a recoverable visual workflow; rights/provenance requires exact
subjects and explicit candidate/approval/selection separation. These findings
are not independent review, human approval or an implemented integration.
