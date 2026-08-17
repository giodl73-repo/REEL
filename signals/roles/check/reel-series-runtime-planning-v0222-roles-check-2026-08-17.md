---
skill: roles-check
topic: reel-series-runtime-planning-v0222
date: 2026-08-17
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.22 comprehensive series-planning role review

## Artifact identification

- Type: Rust CLI/code, additive series contract, audit report, tests, and
  operator documentation.
- Domain signals: episodic story planning, edit rhythm, narration and silence,
  visual cadence, platform variants, continuity, review, and deterministic
  production lineage.
- Reviewed artifacts: `src/series.rs`, the CLI surface in `src/main.rs`, the
  episodic-series template, tests, v0.2.22 guide, and a live audit of BERTICA's
  five-season/fifty-episode index.
- Product boundary: REEL is a portable planning/orchestration grammar, not a
  writers' room, shooting scheduler, asset SaaS, or NLE replacement.

## Role selection

- Animation Director — runtime budgets must mature into feasible visual cadence.
- Editor — episode range, rhythm, exception, and revision behavior are central.
- Sound Designer — narration, poems, pauses, and score occupy the planned time.
- Story Director — duration must serve arc rather than flatten it.
- Platform and Audience — runtime, accessibility, review, and derivative cuts
  vary by delivery context.

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Named visual-breathing budgets are not reconciled with conformed shots, holds, or beat markers. | P2 | `RuntimePlan.components_seconds` | Add planned-versus-measured component reconciliation from child manifests. |
| 2 | Total-runtime drift cannot show whether an episode is visually frantic or static. | P3 | `EpisodeTimingAudit` | Report shot count, median hold, long-hold share, cuts per minute, and motion/hold mix. |
| 3 | Timing and asset readiness remain separate reports. | P3 | audit boundary | Summarize missing selected media, unresolved picture locks, and render readiness without duplicating their source contracts. |
| 4 | One episode plan cannot express materially different landscape and social-cut rhythms. | P3 | runtime scope | Model platform derivatives as separately identified plans or series variants. |
| 5 | The audit projects duration but not render cost or asset-generation burden. | P3 | season projection | Add optional shot/asset/render workload summaries after actual manifests exist. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Short/standard/long ranges are repeated inline, so nominally identical classes may drift. | P2 | `RuntimePlan.class` | Add shared series runtime profiles referenced by ID, with episode overrides explicit. |
| 2 | Under/over findings have no waiver reason, owner, review state, or decision reference. | P2 | `range_status` | Add an exception sidecar or index so intentional finales and unresolved overruns differ. |
| 3 | The audit has no prior-report comparison, so pacing changes across revisions are not summarized. | P2 | report lifecycle | Bind reports to series hashes and add timing-audit comparison. |
| 4 | Neighbor deltas alone do not describe the season's distribution. | P3 | series/season summary | Add shortest/longest, median, quartiles, spread, and class-level distribution. |
| 5 | The live BERTICA audit finds 0 planned episodes, an 18:04:54 raw projection, and 24 adjacent changes above 35 percent. | P3 | consumer adoption | Calibrate short/typical/long episodes before assigning all fifty ranges. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Planned poem, narrative, silence, and sound budgets are arbitrary names with no measured reconciliation. | P2 | `components_seconds` | Derive actual compatible components from narration cues, pauses, production units, and audio events. |
| 2 | Series reporting does not separate narrator, poet, and cast-character duration or pace. | P2 | episode audit | Aggregate speaker/mode duration from voice-consistency evidence without exposing private text or paths. |
| 3 | Unmeasured narration and pauses were emitted as 0.0 percent against raw estimates. | P2 | `share_percent` | Emit unknown until positive measured evidence exists; amendment applied. |
| 4 | Mix density, score-free comparison state, ducking coverage, and audio-check status are absent. | P3 | audit integration | Summarize existing audio evidence by reference rather than reimplementing sound validation. |
| 5 | Free-form component labels invite `poem`, `poetry`, and `Andresito` to become incomparable categories. | P3 | component vocabulary | Let shared profiles define canonical component IDs while retaining display labels. |

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Runtime classes carry no shared story function, so consistency can become numerical rather than dramatic. | P2 | runtime profiles | Let profiles name editorial intent such as threshold, journey, chamber, rupture, or finale without enforcing a universal taxonomy. |
| 2 | The audit cannot relate duration to source density. | P3 | source/runtime synthesis | Report selected source units or words per projected minute as an orientation warning. |
| 3 | A justified long-to-short transition is warned exactly like an accidental one. | P3 | neighbor drift | Allow an episode transition rationale or approved exception while preserving the raw delta. |
| 4 | Hook, poem threshold, climax, aftermath, and landing have no explicit sub-budget semantics. | P3 | component planning | Define project-owned structural component IDs in shared profiles. |
| 5 | Adjacent drift resets at season boundaries, so finale-to-premiere rhythm is invisible. | P3 | season loop | Add an optional cross-season transition audit for continuous-release series. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The command's text mode still prints JSON, making a fifty-episode audit difficult to review. | P2 | CLI presentation | Add concise table/Markdown output and retain JSON for automation. |
| 2 | Runtime plans are not platform-qualified. | P2 | series contract | Keep one primary program plan and link separately identified podcast, YouTube, trailer, and social derivatives. |
| 3 | Caption accessibility and audio quality gates exist but are not visible in the timing overview. | P3 | audit aggregation | Include status references and counts without leaking captions or media paths. |
| 4 | REEL has no real-time comments, assignments, permissions, or client-facing review interface. | P3 | product boundary | Integrate with Git and review platforms; do not build a weak replacement inside the manifest core. |
| 5 | Release cadence, calendar, audience retention, and completion analytics are outside the report. | P3 | distribution boundary | Add importable evidence adapters later, keeping editorial targets distinct from observed audience data. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0 | P2 issues: 10 | P3 notes: 15

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: REEL now has a sound runtime-planning primitive, but comprehensive
series consistency needs shared profiles plus planned-versus-measured component
reconciliation.

Cross-role consensus: do not pursue equal episode lengths. Make intended class,
story function, delivery target, measured evidence, and exception rationale
explicit enough that variation is deliberate and reviewable.

## Amendments

1. Add shared runtime/pacing profiles with canonical component IDs and explicit
   per-episode overrides; this removes repeated ranges and class-name drift.
2. Add measured component reconciliation and a human-readable season dashboard,
   including distribution and source-density orientation.
3. Add hash-bound audit comparison and exception governance before calling the
   planning layer production-comprehensive.

## Amendment applied during review

`share_percent` now emits unknown when narration or pause duration has not been
measured, and a regression test protects that distinction.

## Validation evidence

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all`
- `py C:/src/tracker/repos/standards-protocols/roles/tools/check_roles.py .`
- Live BERTICA audit: 50 unplanned episodes, raw-orientation projection
  `65,094,000 ms`, and 24 neighboring changes above the 35 percent threshold.
