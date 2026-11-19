# Repair follow-up and technical resolution proposals

`reel_workflow::resolution::propose` connects a checked baseline report,
exact-pin repair/revision, corrected reports and an explicit follow-up decision.
It recomputes the candidate plan through `repair::materialize`; stale report,
plan, commit, project, format or language bindings refuse.

The decision uses `reel.workflow-resolution-decision.v1`. Its fields are:

- `baseline_report_sha256` and `candidate_plan_sha256`.
- `contexts`: language to exact corrected `report_sha256`, required
  `production_task` in the affected rebuilt plan, and
  `unit_modalities` (unit key to nonempty, unique inspected modality list).
- `unit_map`: every original-language unit key to a distinct corrected key.
- `evaluations`: one entry for every repaired finding, containing its
  `finding_key`, `corrected_unit`, explicit `rationale` and corrected `evidence`.

All languages affected by the repair, plus the original report's language, must
have a corrected report and declared context. Coverage must name every corrected
unit, including new neighboring units. Comic context requires a whole-page unit
for each page; existing page order must remain a subsequence of the corrected
edition. Film ranges must cover the whole corrected timeline with no gaps.
Comic/audio and film/layout modality mismatches refuse.

Explicit unit mapping supports changed frame ranges and keys while preserving
producer cel/shot/cue or page/panel/balloon identity. It cannot silently drop old
targets, collapse mappings or rename a target to claim a fix. Finding evaluations
must declare the original modality at the corrected target and cite the corrected
review asset (optionally its publication descriptor). Old or unrelated evidence
refuses. A same-code finding at the mapped identity blocks the proposal, including
a finding filed against an aliased key for that same identity. Other findings
remain recorded as outstanding; they are not implicitly closed.

An empty corrected report is insufficient without the explicit coverage and
per-finding declarations. These declarations are authored assertions, not proof
that a person or model viewed/listened to the material. They grant no creative,
translation, rights, selection or release approval. Targets removed entirely or
editions reordered need a future explicit policy; the current contract refuses
instead of treating absence as resolution.

## Native retained adapter

```text
reel-workflow-native export-resolution BASELINE_RETAINED_REVIEW PLAN REPAIR REVISION REBUILD_BINDINGS CORRECTED_RETAINED_REVIEWS DECISION NEW_PROPOSAL --apply
```

A retained-review bundle embeds `packet`, `source`, `admission`, `receipt`,
`result` and `acknowledgement`, using their existing native schemas. Corrected
bundles are keyed by language. `REBUILD_BINDINGS` is the existing graph binding
map for the materialized candidate plan, including dependencies needed to verify
its affected tasks. Every required rebuild must be observed Retained through the
native graph verifier; an absent, blocked, stale or Waiting rebuild refuses.

The adapter verifies complete native baseline and corrected report bytes and
clones only their checked semantic values. It releases each proof before opening
the next, because reviews may share retained actors or object gates. The private
proposal records report/result/acknowledgement hashes, original task/attempt,
rebuild task keys and the exact graph-binding digest. It is a sequential historical
observation, not an atomic custody snapshot or a reusable live guard.

Each context now names its exact producer task. After the retained graph check,
the adapter reopens that original task's result and acknowledgement and derives
its review copy from the retained comic/film producer proof. Every field of that
derived copy must equal the corrected report's copy: project, production commit,
language, publication/review identities and bytes, format, units and ranges.
The same commit with a different review asset is insufficient. The proposal
records producer task/result/acknowledgement/copy digests per language. These
proofs are released sequentially to avoid reentrant actor or object gates.

Output is a new, exclusive private control file. The command checks the declared
native roots, source repositories and rebuild control directories, holds its
output parent and rebuild profile files while writing, and preserves partial
writes. It performs no admission, provider call, rendering, selection, tracker
message/closure, job close or scratch retirement.

## Scope and qualification

Component tests cover film/comic bilingual context, explicit evaluation, stale
edition/evidence refusal, missing pages/timeline coverage, modality checks,
persisting findings through aliases, unrelated outstanding findings, retiming and
page reordering. Native adapter compilation and standard regression checks are
separate from an end-to-end repaired-production qualification.

The native fixture connects the existing retained corrected comic
production, a new retained contextual follow-up report and the original finding
to this exporter under their exact source/operation identities. It must check
that the reviewed material corresponds to the retained repaired outputs, not
merely the same source commit. The fixture now passes for the bounded corrected
comic case described below. Retention/adoption of the resolution itself,
tracker close effects and their
uncertainty reconciliation remain open. A private proposal alone is never issue
closure evidence.

Applied comics-editor, editor and rights/provenance checklists: full neighboring
context, stable producer identities, language-local timing and evidence, old
history preservation and separate technical/creative authority. No independent
review or actual approval is claimed by this feature.

## Component and regression evidence

Source `36af09c9411bcfb8e30c0b51503ab0e1ea49306a` has all 1,913 tracked
files verified against its archived source snapshot. All 98 workflow tests,
61 standard native tests and 247 unchanged SDK source pins pass, with strict
workflow/native Clippy (native film and default features). Formatting and
`git diff --check` pass. Four new parameterized resolution tests cover the
contract cases above; the native exporter is compiled and included in standard
regression checking. This initial snapshot had no integrated positive
qualification; the subsequent lineage evidence follows below.

External evidence: `F:/src/tig-dev-temp/reel-resolution-20261119-v1/qualified-tools.json`,
SHA256 `f5502892a10517ba6ad720d6cc02518529a625e14b538c943cbf4999503e99cd`.
It pins the source/archive, current frozen native CLI/test executables and check
logs. No new feature execution admitted jobs, called providers, rendered media,
published native outputs, selected assets or closed issues in this qualification.
This initial component evidence precedes the integrated evidence below;
passing component tests do not substitute for integrated qualification.

## Corrected comic lineage evidence

Source `de6f065caf0fbc03c56c64b8b76932012b83dc28` has all 1,914 tracked
files verified against its archived snapshot. All 98 workflow tests, 62 standard
native tests and 247 unchanged SDK source pins pass, with strict workflow/native
Clippy (native film and default features). The new native unit test refuses a
different review asset at the same production commit, changed publication
identity or changed context units.

The integrated fixture passes in 265.46 test seconds / 266.17 wall seconds.
Its preserved follow-up root is `F:/src/tig-dev-temp/.tmpkHoGyv`, with source
`F:/src/tig-dev-temp/.tmpZPKcjk`. It reuses the original retained finding at
`.tmpYT3pCe` and corrected comic at `.tmpWZZmCD`, admits a new contextual report,
publishes only its three declared controls and retains the report under its
original attempt. It then exports one private resolution proposal whose producer
copy exactly matches the retained corrected render. Baseline and producer claim
files remain byte-identical across execution.

The report and per-unit Text evaluation are scripted technical declarations.
They do not establish actual viewing, translation/creative approval or issue
closure. No provider call or replacement production job runs; historical
producer proof verification may reconstruct reference pixels. The follow-up job
remains protected, and all original roots, runtime pins and immutable bytes are
preserved.

A read-only counterexample supplies the original historical report as the
corrected report while naming the retained corrected producer. It refuses with
`resolution-production-review-copy` in 101.72 wall seconds, creates no output
proposal and leaves baseline, producer and follow-up claim files unchanged.
The same-commit/different-media counterexample is additionally covered by the
native unit test; the historical counterexample exercises the complete exporter
against actual retained jobs.

External evidence: `F:/src/tig-dev-temp/reel-resolution-lineage-20261119-v1/qualified-tools.json`,
SHA256 `03caa019ddaa610eb85831f2eada1f2400ffff661fa0d1f4a2fb1afb210dc90e`.
It pins source/archive, frozen tools, check/fixture/refusal logs, original
operation controls, producer controls and private proposal/decision bytes.
Qualification is Windows/NTFS, the existing synthetic single pool, one Spanish
comic page and one original finding. Film, full issues, multiple-language native
resolution, project adoption and actual model/creative inspection still need
their own evidence. Retention/adoption of resolution proposals and tracker-close
effects/reconciliation remain open.
