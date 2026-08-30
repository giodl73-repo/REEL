---
skill: roles-check
topic: reel-showrunner-v023
date: 2026-08-17
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# Role check — REEL showrunner control v0.2.23

## Artifact identification

- Type: Rust schema, CLI, audit reports, sanitized fixtures and integration
  documentation.
- Primary artifacts: `src/showrunner.rs`, the five `showrunner-*` commands,
  `docs/showrunner-control-v0.2.23.md`, and sanitized acceptance tests.
- Boundary: the feature validates human-authored declarations; it neither owns
  source canon nor simulates creative or release approval.

## Selected roles

| Role | Reason |
|---|---|
| Story Director | The contract models promise, question, pressure, consequence, revelation and finale delivery. |
| Editor | The audits evaluate episode order, tonal transitions, intensity and cross-episode rhythm. |
| Platform and Audience | Audience assumptions, orientation, report legibility and human review are first-class. |
| Animation Director | Production scale, optional load, asset reuse and series continuity affect feasibility. |
| Sound Designer | Narrator distance, knowledge handoffs, tonal bridges and silence must remain structural. |

## Findings

### Story Director

| # | Finding | Severity | Section | Recommendation |
|---:|---|:---:|---|---|
| 1 | Generic audits must not manufacture dramatic questions or canon. | P2 | Ownership boundary | Require upstream strings and keep every creative weakness advisory. **Applied.** |
| 2 | Requiring a function family somewhere in a season does not prove the premiere or finale occupies the boundary. | P2 | Season controls | Add optional opening/closing function declarations and mismatch findings. **Applied.** |
| 3 | A revelation that develops across several films should not be collapsed into a single falsely precise instant. | P2 | Revelation steps | Add non-overlapping `through_episode_id` spans and preserve them in reports. **Applied.** |

### Editor

| # | Finding | Severity | Section | Recommendation |
|---:|---|:---:|---|---|
| 1 | Uniformity assumptions would punish intentional compact films, grief holds or unconventional seasons. | P2 | Audit policies | Run only project-declared adjacency, boundary and transition policies. **Applied.** |
| 2 | Tone labels alone do not explain a difficult transition. | P3 | Tone control | Preserve an authored bridge field and warn only for configured abrupt pairs lacking one. **Applied.** |
| 3 | Reports must be deterministic for review diffs and CI. | P3 | Reports/tests | Use ordered maps/vectors and prove identical serialized audits across repeated runs. **Applied.** |

### Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---:|---|:---:|---|---|
| 1 | Showrunner validation could appear to replace platform, caption, privacy or delivery validation. | P2 | Documentation | State that `series-validate` remains a separate production gate. **Applied.** |
| 2 | A review queue must never fill in a named human's opinion. | P2 | Review queue | Read only existing series findings/status; missing reviewer evidence remains open. **Applied.** |
| 3 | Machine output must not expose a local series path. | P3 | JSON reports | Report IDs and binding hash only; the acceptance test rejects serialized temporary paths. **Applied.** |

### Animation Director

| # | Finding | Severity | Section | Recommendation |
|---:|---|:---:|---|---|
| 1 | A dramatic `historical-public` or `spectacle` label is not a cost estimate. | P2 | Production portfolio | Add optional explicit load complexity, locations, roles, crowd, new assets and reusable assets. **Applied.** |
| 2 | Projects without estimates must not receive invented production warnings. | P2 | Production policies | Audit high-load clusters only when both estimates and an explicit threshold/maximum exist. **Applied.** |
| 3 | Character/place/object continuity already belongs to the series and manifest layers. | P3 | Schema boundary | Reference the bound series instead of duplicating continuity entities in the showrunner sidecar. **Applied.** |

### Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---:|---|:---:|---|---|
| 1 | Immediate narration can accidentally carry later knowledge even when reveal order is valid. | P2 | Viewpoint audit | Declare immediate distances and later layers; warn when no audible/editorial handoff is recorded. **Applied.** |
| 2 | Emotional intensity must not become an instruction to add music or increase loudness. | P3 | Tone/intensity | Treat intensity solely as an adjacency index; leave cue performance and mix decisions in their existing layers. **Applied.** |
| 3 | A tonal bridge may be silence, ambience, narration breath, picture or music. | P3 | Transition control | Keep the bridge as an authored string instead of imposing an audio implementation. **Applied.** |

## Synthesis

Roles reviewed: 5  
P1 blockers: 0 | P2 issues: 9 | P3 notes: 6

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: REEL must make series-level audience structure inspectable without
becoming the author of that structure.

Cross-role consensus: normalized indices and deterministic reports are useful
only when exceptions, human authority and the distinction between dramatic
scale and actual production load remain explicit.

## Amendments

1. Added season-boundary function audits and revelation spans so the tool tests
   actual order rather than mere presence or falsely instantaneous reveals.
2. Added optional production-load and reuse fields with opt-in cluster policies;
   dramatic scale alone never invents cost.
3. Clarified that `series-validate` remains required, preserved path-free stable
   JSON, and kept narrator handoffs, sound implementation and all human findings
   outside automatic creative inference.

Conditions remaining: upstream projects must author truthful controls, run their
own source/history/privacy gates, and obtain actual human review. Those are not
code defects and must not be auto-resolved by REEL.
