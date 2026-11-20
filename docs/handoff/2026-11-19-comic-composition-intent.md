# Comic composition intent and producer correspondence

Comic issue assembly v2 can bind actual ordered parts to the authored workflow
composition intent. This is an additive producer check, separate from the
[composition planner](2026-11-19-composition-planner.md)'s metadata accounting.
Film producer integration and full episode-scale qualification remain open.

```text
reel-workflow export-composition-intent DECLARATION NEW_INTENT
```

The command checks the complete declaration and writes a new private
`reel.workflow-composition-intent.v1` control. It contains project, format and
each edition's language, ordered component task IDs, other consumed control
parents and final task ID. It deliberately excludes recipe/request hashes and
source commits: the exported intent can be pinned into a request before the
recipe is finalized, without creating a circular content hash. Exact producer
versions remain bound by the task's native/upstream declarations and original
parent packets/results. The intent is neither an approval nor a custody proof.

## Versioned request

An assembly with schema `reel.comic-issue-assembly.v2` requires a `composition`
object containing an exact request-relative `intent` Pin (path, SHA256, length)
and a `producers` map from each authored part ID to its workflow producer task ID.
The map must cover exactly the issue plan's parts. Following the plan's actual
part order through that map must reproduce the intent's ordered component IDs.
The publication edition and language must match the intent. Explicit part IDs
are required in v2 part requests/receipts.

The workflow binder additionally requires the intent to be a named task input
with an exact source binding, and checks the intent project and final task ID
against the packet. It accounts for these bytes in the existing comic input
closure. The renderer reads and verifies the pinned intent itself, checks the
actual issue plan and deterministically replays every part. The exported editable
issue carries `composition-intent.json` and a relocated assembly request, so
reassembly needs neither the original recipe nor the worker source folders.

The managed native adapter requires live admitted input authority and frozen
upstream origins for v2. It checks each part receipt against its original producer
using the WorkerReceipt selector, each editable carrier against that producer's
`editable.packet` output selector, and each individually supplied output against
its exact output selector. All consumed producer/control parents must equal the
declared edition's component/control parent set. Equal bytes from a different
producer do not satisfy this check. Production, returned-result checking and
native historical replay share this validation path.

Legacy assembly v1 omits `composition`; serialization preserves the existing
request bytes. A v1 request with composition or a v2 request without it refuses.
The derived `ComicIssueControl` producer also supports an explicit v2 request;
see [derived intent controls](2026-11-19-derived-intent-controls.md). Existing v1
pipelines retain their exact request/output bytes rather than being upgraded
implicitly.

## Validation and producer checklist findings

The synthetic comic renderer fixture exercises three actual pages in two parts,
full editable-source replay, a pinned v2 intent and reassembly from the captured
packet. It refuses swapped producer order, changed intent bytes, wrong language
and mixed version envelopes. Workflow tests cover project/final-task binding,
named intent input requirements, no-clobber CLI export and intent independence
from recipe prompt bytes. The native origin matcher unit test covers receipt and
bundle selectors, substituted producers, missing carriers and missing controls.
Those unit tests do not establish a managed v2 admission/retention lifecycle.

Validation passes: 128 workflow tests, 16 publication tests including the rendered
comic fixture, and 85 standard native tests (67 ignored fixtures excluded).
Strict all-target Clippy passes for workflow, publication with comic support and
native with/without film. All 247 vendored Tig source hashes/lengths still match;
`git diff --check` passes. Local test logs are retained under
`F:/src/tig-dev-temp/reel-composition-intent-{workflow,publication,native}-tests-20261119.log`.
These are local component/regression results, not a qualified v2 native runtime
distribution or a real BBJ/S5 adoption.

Using the comics editor, story director, editor, platform/audience and
rights/provenance checklists under `.roles/ROLE.md`, plus the additive-contract
and hash-lineage invariants in `.pitfall/reel-invariants.md`:

- Ordered parts, page IDs and spreads remain governed by the checked issue plan;
  intent changes do not silently reorder pages or change their selected sources.
- The retained editable packet includes the intent and still reconstructs art,
  lettering, fonts and page order. Digital reading output remains distinct from
  printer approval, canon/translation approval and release authority.
- Native original-parent checks add provenance correspondence; approval, selection,
  retained ownership and completion still require their existing separate gates.

No independent reviewer or human approval is claimed. No existing S4/S5 job,
recording, approval, selection or owned runtime profile is changed. The preserved
large-native ImportPending job is not retried. The Tig SDK remains unchanged.

Remaining qualification: managed v2 comic lifecycle
and post-close history, project-scale comics, film/conform order binding, full
bilingual S5 composition/delivery and owned profile adoption/rollback.
