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

## Scope and next qualification

Component tests cover film/comic bilingual context, explicit evaluation, stale
edition/evidence refusal, missing pages/timeline coverage, modality checks,
persisting findings through aliases, unrelated outstanding findings, retiming and
page reordering. Native adapter compilation and standard regression checks are
separate from an end-to-end repaired-production qualification.

The native fixture connects the existing retained corrected comic
production, a new retained contextual follow-up report and the original finding
to this exporter under their exact source/operation identities. It must check
that the reviewed material corresponds to the retained repaired outputs, not
merely the same source commit. End-to-end qualification is pending until that
fixture passes. Retention/adoption of the resolution itself,
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
regression checking, but has no integrated positive qualification yet.

External evidence: `F:/src/tig-dev-temp/reel-resolution-20261119-v1/qualified-tools.json`,
SHA256 `f5502892a10517ba6ad720d6cc02518529a625e14b538c943cbf4999503e99cd`.
It pins the source/archive, current frozen native CLI/test executables and check
logs. No new feature execution admitted jobs, called providers, rendered media,
published native outputs, selected assets or closed issues in this qualification.
The next integrated fixture and remaining closure boundaries are listed above;
passing component tests do not substitute for them.
