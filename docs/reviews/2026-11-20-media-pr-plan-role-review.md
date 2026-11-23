# Media expansion PR plan: role-checklist review

**Subject:** [Phased PR plan](../media-expansion-pr-plan.md) at
`7c2d4a56b5b3f4d736f6666e2951a21b566975a2`, with corrections recorded below.
**Method:** one assistant applying all nine [REEL role checklists](../../.roles/ROLE.md)
to planning requirements and evidence. No independent reviewers, actual media
audition, device qualification, creative approval or rights clearance is claimed.

Consulted the prior [spoken-plan review](2026-11-20-spoken-plan-role-review.md),
the checklists, [contributor checks](../contributing.md) and the phased plan.
This reviews proposed gates, not source implementation of future capabilities.

## Findings and corrections

**Medium** findings could allow an ambiguous or unsupported completion claim;
**Low** findings affect tracking clarity. No implemented runtime is scored here.

| ID | Severity / lenses | Baseline finding | Disposition |
| --- | --- | --- | --- |
| P01 | Medium — Editor, Sound, Platform/Audience | Output limits, mix checks, tolerances and runtime commands were required only generally; "check" had no uniform observable result. | **Closed in plan:** linked normative test matrix, per-PR evidence fields and M00/M11/M13 threshold/command pinning before runtime implementation. |
| P02 | Medium — Music Reconstruction, Sound, Rights | Independent checking lacked a specified neutral PCM proof, container/signal distinction, portability rule and adversarial rehashed-report case. | **Closed in plan:** exact lossless oracle, declared lossy policy, Windows/Linux core coverage and independent reopening/tamper tests. Missing runtime stays not run. |
| P03 | Medium — Editor, Rights, Platform/Audience | Stop-on-failed-gate and later engineering after M06 were open to conflicting interpretations; status instructions also allowed updates only after merge. | **Closed in plan:** engineering dependencies and human/adoption gates explicitly separated, with in-progress links and separate recorded states. No inherited listening acceptance. |
| P04 | Medium — Story, Vocal Adaptation, Sound | Naturalness/word fidelity/context review had no common human rubric, and dependency validation might be overread as factual or language verification. | **Closed in plan:** complete-programme/per-join rubric, language-specific decisions and explicit machine-versus-human boundaries. |
| P05 | Medium — Editor, Sound | M06 first introduced a multi-cue case without ensuring the earlier contract represented it; selective-change checks could conflate aggregate and consumed identities. | **Closed in plan:** multi-cue contract support in M01 and qualification fixture in M06; M05 compares consumed recipes separately from changing aggregate indices. |
| P06 | Medium — Score/Arrangement, Music Reconstruction, Vocal Adaptation | M09's optional score language allowed a diagram-only lesson to close the planned score/songbook slice. | **Closed in plan:** required editable score fixture with pickup/repeat traversal, explicit musical/sample anchors and stale/ambiguous mapping rejection. |
| P07 | Medium — Rights, Platform/Audience | Privacy-safe evidence and side-effect-free validation lacked concrete regression expectations. | **Closed in plan:** controlled canary redaction tests, no implicit execution/egress/storage effects, exact declared local-output exception and explicit failure/no-promotion expectations. |

## Role coverage after correction

| Checklist | Plan assessment | Production evidence still required |
| --- | --- | --- |
| [Story Director](../../.roles/story-director.md) | Summaries preserve declared context/qualifications; M13 branches are authored; automatic checks cannot establish factual fidelity. | Actual scripts/branches and source/narrative authority decisions. |
| [Animation Director](../../.roles/animation-director.md) | M10 names semantic transitions, independent language clocks, frame/sample and alpha-source checks; later visual targets have declared profiles. | Visual feasibility, continuity/style and actual decoded-frame/device evidence. |
| [Editor](../../.roles/editor.md) | Bounded slices, explicit joins and per-PR test/review identities now make completion inspectable. | Actual pacing, full-programme joins and omission/reordering review. |
| [Sound Designer](../../.roles/sound-designer.md) | Neutral/processed sample oracles, numeric profile choices, handoff rubric and playback limits are required. | Real speech/mix listening; controlled signals do not establish naturalness. |
| [Platform and Audience](../../.roles/platform-audience.md) | Target profiles, accessibility, keyboard/text and real-device evidence cannot be inferred from unit tests. | Selected target briefs, actual accessibility/usability and device checks. |
| [Music Reconstruction Engineer](../../.roles/music-reconstruction-engineer.md) | Source/signal identities, unchanged regions, rounding, portability and ambiguous score mappings have explicit tests. | No source separation/repair is included; any later such work requires its own scope and neutral-reassembly proof. |
| [Score and Arrangement Director](../../.roles/score-arrangement-director.md) | M09 cannot close without editable-score, pickup/repeat and export-binding evidence; PDF remains derivative. | Musical usefulness, performed/model correspondence and actual musical decisions. No new arrangement approval. |
| [Lyrics and Vocal Adaptation Editor](../../.roles/lyrics-vocal-adaptation-editor.md) | Text/performance/recognition/translation remain separate; language-specific human review and uncertainty are explicit. | Actual authorized texts and wording/prosody/pronunciation review. |
| [Rights and Provenance Steward](../../.roles/rights-provenance-steward.md) | Scope, redaction, no hidden execution, immutable retained inputs and distinct decision gates now have concrete test expectations. | Actual authority, consent and provider evidence; technical validity grants none. |

The main tradeoff is retained: exact reuse makes production efficient, while
meaning and natural joins may require a new bridge or performance. Similarly,
short CI fixtures keep PRs bounded but cannot replace actual listening/device
review. Both tensions remain visible rather than being resolved by lowering gates.

## Validation and remaining work

Checked local Markdown links/anchors in the changed plan, test matrix and review
record; `git diff --check` passes. The plan's diagram is unchanged. Required hosted
documentation checks apply before landing. No test harness was added for prose.

All seven identified planning findings are corrected. Exact symbols, command
names, core fixture values and runnable test IDs are M00 deliverables; later
player profile values are M11/M13 deliverables. Their absence today is explicit
future work, not a completed technical gate. None of the implementation tests,
listening or device checks in the matrix was executed by this documentation review.
