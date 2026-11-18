---
work: workflow-orchestration
stage: panel
format: production-workflow
style: renderer-neutral
source_scenario: project-owned
author: producer
rubric_version: v0.1
created: 2026-11-17
updated: 2026-11-17
sources: [docs/workflow-orchestration.md, .roles/ROLE.md]
---

# Workflow orchestration producer role review

Target: docs/workflow-orchestration.md, docs/publication.md, formats/comic-issue.md,
crates/reel-publication and crates/reel-workflow, additive to
REEL base 3954171a53086973f3160cd033b27524fa359008. Method: source/contract
inspection, failure scenarios and targeted synthetic tests. Applied REEL's
nine craft checklists and Melena's six technical checklists supplied by the
producing project. These are producer lenses, not delegated/independent reviewers
or human approval. No private media, actual provider or production store is used.

| Lens / severity | Finding | Owner / disposition and evidence |
| --- | --- | --- |
| Story / Animation, high | Valid generation can violate canon, current references or requested cadence. | Project policy + REEL W5: immutable input references; separate school/performance/technique and shot/exposure timing. Actual visual-context acceptance remains open. |
| Editor / Platform, high | Minimal rebuild can omit neighboring context; review/delivery formats differ. | REEL W5: separate rebuild/review scope, project-specific derivatives and full-film checks. No current render claim. |
| Sound / Reconstruction, high | New take changes clocks and dependent mixes. | Existing assembly/audio engines + W5: preserve selected takes, exact alignment and protected-region checks. Listening/neutral reconstruction remains operation-specific. |
| Lyrics / Score, high | Input text/transcription does not prove performed wording or authorize adaptation. | Upstream text/music authority: separate hashed layers and decisions; activate deeper checks on changed layers. No musical approval claim. |
| Rights / Provider privacy, blocking before real dispatch | Handoff is also egress; returned code/identity may be untrusted. | W1/W5: same permitted context in both modes, private packets, unknown provenance explicit, code review. W0 makes no dispatch or provider-authentication claim. |
| Identity, high | Task ID alone accepts stale or wrong-language results. | W0: task fingerprint binds exact project/source/task/input/dependency/output contracts. Direct and handoff use shared checks; logical/source/review IDs remain separate. |
| Concurrency / Recovery, blocking before lifecycle adoption | Duplicate effects and old-operation replay can corrupt task accounting. | W1/W2: preserve original operation/WorkRef/activation; exact replay/conflict checks in W0 do not claim durable idempotency. Lost replies propagate NeedsRecovery without auto retry. |
| Placement / Custody, blocking before real staging | Wait/timeout does not relinquish claims or prove retained outputs. | Existing Tig lifecycle + W1/W2: expose actual 16-actor table/128-change limits; retain/reopen/owner ack before completion. No automatic expiry, deletion or custody inference. |
| Git / Adoption, blocking before selection | Detached proposal is not expected-ref selection CAS. Qualified source includes uncommitted changes. | W3: explicit baseline transaction and new-path handling. Pin source manifests; do not infer latest API from ancestry. W0 changes no SDK binding or selection. |

Preserved tension: engineering limits rebuilds to changed dependencies; craft
review inspects neighboring sequence/full required outputs. Both remain required.

Design disposition: proceed with bounded W0 contracts. W1-W5 remain explicit
qualification gates; this record approves no production, source selection,
translation, voice, private/public delivery or cache retirement. See the work
package table for remaining fixtures. Validation results belong in the handoff;
test design alone is not counted as execution evidence.

W0 local execution: ten synthetic integration tests pass, including the actual
CLI export/no-clobber/result-byte checks and a fake direct adapter. Targeted
locked offline Clippy with `-D warnings`, workspace rustfmt check and Git diff
whitespace check pass. The consumed external SDK's 247 source-manifest entries
were hash/length verified with zero mismatches; this did not install or adopt it.
Cargo reported inability to update its global last-use database on the full D
volume; compilation and tests succeeded using the existing shared C target.
Hosted CI, native lifecycle acceptance and other work packages remain untested.

Comic-format follow-up supersedes that W0 count: eight publication and twelve
workflow synthetic integration tests pass. Named output-set validation is shared
between crates; workflow v2 refuses the earlier single-output schema. Tests cover
film references, language-local spatial order, duplicate IDs, silent/cover pages,
spread pairs, required/unknown/conflicting output sets, output allowance overflow,
actual read-only publication CLI and existing workflow CLI refusals. Locked offline
tests, targeted strict Clippy, workspace fmt and tracked/new-file whitespace checks
are the required final checks. No actual comic page render or print claim follows.

Applied the new Comics Publication Editor lens with story/editor/platform/rights:
page-turn and language-fit review, semantic layer ownership and fresh lettering
restoration remain adapter acceptance gates. The new metadata checks cannot prove
those qualities. Existing BBJ comic/print lenses informed this requirement; BBJ's
roles, active files, frozen tools and asset/runtime bindings were not changed.

Comic producer follow-up: `comic` is an explicit opt-in feature. A new bounded
page author assembles separately pinned PNG layers, rectangular panels and exact
utterance geometry, reusing reel-print's existing font/SVG machinery plus its
new in-memory lossless raster/PDF page entry. Default metadata checks retain
their inert scope. The new worker's render/check/receipt-pinned restore are
explicit operations over owned development packets, not native publication.

Actual local tests: five print regressions, two comic production/restoration
fixtures, eight publication contracts and twelve workflow contracts pass (27
total). The persistent synthetic fixture writes ES, EN, a bounded Spanish text
fix, an exact source-restored rebuild and a new edit from restored sources.
All ten PDF pages independently rasterize to exact selected PNG RGB bytes;
all six distinct page appearances were visually inspected. Existing source
unavailability, restored editable lettering, protected clean art/unmodified page
and English outputs, stale pins/path traversal, balloon fit and existing-output
refusals have actual evidence. No end-to-end native handoff/retention, arbitrary
panel geometry, print-ready production, finished illustration, selection or
creative approval is claimed.

Craft disposition: the synthetic pages are legible mechanism proofs with simple
shapes, not publication art. Font advance checks do not replace actual text fit;
role assignments do not prove semantic layer ownership. Native custody/adoption
findings stay open. Build/cache capacity on C/D required using the existing F
build target and temporary root; no asset deletion or cache migration was used.

Deterministic binding follow-up, producer-applied comics/editor/rights lenses:
exact logical page identity, language-local order, all source pins and output
namespace are now checked against the selected edition. A byte-identical logical
alias is refused. Tool returns cannot claim approval, print readiness or native
retention through their receipt. The local CLI explicitly leaves snapshot binding
unverified; source correspondence is insufficient for custody. Existing v2 LLM
task keys remain stable, and tool tasks cannot enter the LLM adapter.

Execution evidence: 38 targeted tests pass, including ten new binding tests and
a prior-key compatibility test. A separately invoked frozen tool reproduces all
twelve earlier fixed-edition outputs; two written PDF pages independently match
PNG RGB and were visually inspected. Synthetic inputs/zero commit establish no
native lifecycle or project adoption. Native protection, retained-owner recovery,
selection transactions and human creative approval remain open gates. A mutable
build-target tool locator was found and superseded with frozen hash-verified
copies; the historical profile remains unchanged rather than relabeled.

Handoff recovery follow-up, producer-applied editor/rights/custody lenses:
waiting and cancellation must preserve native claims; a lost provider/import
response must preserve the original attempt and operation, not create a second
candidate. The new coordinator records intent before invoking its protected-job
port, verifies retained acknowledgement before recording success, and reconciles
pending work on restart. Partial journals stop without truncation or cleanup.
Local retained markers cannot authenticate native custody; CLI state reports
explicitly leave native verification false. Creative approval, completion,
selection, actor release and scratch retirement remain separate.

Nine coordinator/CLI fault-injection tests supplement the prior 38 tests. These
use a synthetic lifecycle implementation and do not close W1/W2 qualification.
The inspected qualified protection CLI only prepares its existing retained-native
producer profiles. A generic comic adapter must bind exact workflow resources
through the qualified Rust lifecycle, rather than mislabel a film projection or
infer native protection from this control journal. Existing native limits and
historical bindings remain unchanged.

2026-11-18 native input follow-up, producer-applied editor/rights/custody lenses:
the selected logical record and annex leaf must agree in the exact task commit;
finding identical bytes at another path is insufficient. The portable mapping
and stream choices are now pinned in the task identity, while machine locators
remain private. A declaration change invalidates the task key. Bare Git metadata
restores identity without the authored working files. The isolated SDK intake
preserves all 247 qualified upstream pins and leaves existing producer bindings
unchanged; its ancestry commit is not relabeled as a clean release.

Guarded input access is not durable job admission. Native CLI observations keep
that flag false, publish nothing and expose no submitted paths/text. The native
read fixture exercised an existing synthetic store and exact 1 MiB object;
neither the fixture nor the source-binding checks establish retained-owner
acknowledgement, full comic production, creative approval or project adoption.
Source-binding/alias/stream/history tests and all SDK pin checks supplement the
shared suite (57 actual tests including the explicit native read). Actor
acceptance, manual staging, logical registration, recovery and selection
qualification remain open rather than being inferred from successful reads.

2026-11-18 input admission follow-up, producer-applied engineering/editor/rights
and custody lenses: accepted object claims must survive process exit and never
be confused with byte availability, completed production or retained publication.
The workflow adapter now reuses the qualified slot module, binds task/attempt
and original operation, archives exact source metadata and reopens actual native
WorkRef/activation/coverage under the original actor fence. Serialized receipts
cannot grant a guard. Native pending state prevents live verification; recovery
retains original work identity. Cancellation, expiry, release and deletion are
not side effects of these entries.

Six real metadata-journal workflow/CLI fixtures supplement four unchanged original
slot tests and the existing source/byte fixtures. CLI output leaves availability,
retained-owner acknowledgement, completion and selection false; private receipts
require new destinations outside managed roots. Original SDK/module source pins
remain exact. Tight native node/atomic transition limits are surfaced, not worked
around. The 68-test evidence does not combine synthetic claim journals with a
physical production pool or qualify an end-to-end comic handoff. Byte staging,
candidate idempotency, native retained completion and project adoption stay open.

2026-11-18 staging follow-up, producer-applied engineering/editor/rights/custody
lenses: the ticket pins the original admission and concrete directory identities;
staged bytes and exact prompts cannot imply rights, creative approval or retained
output ownership. Preparation is separate from export; original claims and
capacity survive process exit. Resume checks actual native sources and refuses
scratch substitution. Unknown/replaced namespaces, changed files and partials are
preserved, never overwritten or deleted. Initial uncertainty without a complete
ticket requires explicit diagnosis rather than silent adoption or a new job ID.

The 107-test local evidence includes fault-injected filesystem staging, capacity
lease recovery, existing job regressions and a coupled native claim/read/export
fixture across CLI processes. Its cooperating controller/store scope is explicit,
not a claim that provider defaults or all external writers were adopted. Inputs
remain private and exact; no browser package interpretation, provider dispatch,
candidate ingestion, result retention, completion or selection is inferred.
Comic source layout support is not an end-to-end native comic/render qualification.
Native result registration and guarded retirement remain open acceptance gates.

2026-11-18 response-ingress follow-up, producer-applied engineering,
editor, comics-editor and rights/provenance checklists: returned images, text,
audio and opaque browser payloads remain candidate bytes. Whole-set logical
registration cannot be replaced by native content deduplication. Original
task/attempt, admission, artifact identities and supplied provenance are frozen;
changed results cannot silently register another candidate under that attempt.
The native actor preserves original input/runtime history and acquires output
claims before any payload publication. Private controls and return locations do
not appear in portable CLI summaries; provider/model assertions stay unverified.

Fault-injected native Prepare recovery must reconcile only matching original
work. A changed result leaves pending work untouched. Explicit synthetic native
ingress/reopen and a second CLI process verify publication/replay, not retained
ownership, artistic merit, translation, editable comic sources, printer readiness
or public release. Tool/comic responses are refused until their worker-receipt
and native source-correspondence adapter exists. Return files, input claims and
capacity remain preserved. Retained-owner ack, coordinator completion, expected
baseline selection, actor close and guarded retirement remain open. These are
producer-applied findings, not independent reviewers or human approvals.

2026-11-18 retained-acknowledgement follow-up, producer-applied engineering,
editor and rights/provenance lenses: immutable logical registration and published
bytes must be followed by actual retained-owner coverage and native byte
reopening. The adapter now preserves all existing retained paths/root and current
job references/root, validates the exact enrolled target owner and every assigned
source store, and reads back the original native work and positive retained
claims. It does not impersonate the managed scene runner's producer adoption
schema or treat an acknowledgement file as live authority.

Native Prepare recovery preserves original task/attempt/work/evidence. Historical
readback and completed-ack recovery depend on immutable work and retained coverage,
not the former source actor's generation or scratch. Lost coverage and absent
native bytes refuse without selection, cleanup or substitution. Fixtures preserve
unrelated retained history and model source advancement and coverage loss/restore
only through explicit synthetic SDK operations; those are not guarded close or
retirement qualification. Native handoff coordinator completion, tool/comic worker
receipts, selection and retirement stay open. Technical acknowledgement grants no
creative, translation, consent, rights, print or release approval. These remain
producer-applied checklists, not independent review or actual human decisions.

2026-11-18 native handoff follow-up, producer-applied engineering, editor,
comics-editor and rights/provenance lenses: task completion must follow actual
retained native proof, never a deserialized success flag. The adapter now binds
original admission/retained target and the actual running executable, records
intent before effects, reconciles original work and holds verified proof through
completion append. Completed resume rechecks current retained claims and bytes;
corrupt acknowledgements refuse while leaving history intact. Lost completion
does not require a second publication or replacement attempt.

The coupled fixture exposed native object read gates conflicting with new retained
admission. Read handles now close for admission while positive job protection and
ordered actor fences remain, original response coverage is checked even for
staged retries, and output bytes reopen afterward. Passive recovery fencing is
distinct from settled mutation permission. Private control writers/namespaces
are bounded and cooperate through existing locks; partials are preserved.

Technical `Retained` means the task's declared outputs passed byte/custody gates.
It does not approve text, translation, images, voice/likeness, fonts, comics,
printing or release. Tool/comic worker integration, explicit selection and
guarded close/retirement remain open. This is producer-applied review with
synthetic native evidence, not independent reviewers or human approval.

2026-11-18 native comic follow-up, producer-applied comics-editor, engineering
and rights/provenance lenses: a supplied worker receipt is insufficient for
completion. The result now pins it, native registration/retention includes its
object, and the actual source closure and complete rendered packet are replayed.
Page/panel/utterance order, language, source IDs and editable source copies remain
exact. Forged authority and altered rendered bytes refuse before registration.
Historical proof uses original native bytes and bare Git metadata, not a surviving
worker input folder. Missing bytes never become a latest lookup.

The coupled fixture exposed exclusive native gate reuse conflicts. Guarded reads
now share one physical-object handle within the operation only after logical
binding checks; logical artifact roles and exact attempt identity are preserved.
An additional budget gate prevents output protection from growing a job beyond
the qualified clear phase. Large issues need an explicit plan, not hidden phases,
extra actors or history deletion. The full scope remains in the completion ledger;
providers, review/fix integration, selection and retirement are not inferred from
one comic fixture. These are producer-applied checks, not independent reviewers
or creative, rights, translation, print or release approval.
