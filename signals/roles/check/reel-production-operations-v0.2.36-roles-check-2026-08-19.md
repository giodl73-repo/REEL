---
skill: roles-check
topic: reel-production-operations-v0.2.36
date: 2026-08-19
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.36 production operations roles check

## Artifact identification

- Type: Rust contracts, CLI commands, tests, documentation, and portable reports
- Scope: generation evidence, promotion, incremental picture planning, repair queues, portfolio state, voice takes, music provenance, and sprite coverage
- Evidence: `src/production_operations.rs`, `src/sprite_library.rs`, `src/main.rs`, `tests/production_operations_v0236.rs`, and `docs/production-operations-v0.2.36.md`
- Human boundary: this check evaluates technical evidence and production coordination. It does not approve any creative choice, performer, principal, rights position, publication, or release.

## Role selection

- Animation Director: picture materialization, cache reuse, promotion state, and pose coverage affect visual continuity and renderer readiness.
- Editor: timecoded findings, deterministic repair order, proxy disclosure, and exact-byte reuse affect cut review and revision control.
- Platform and Audience: output profiles, dimensions, disclosures, portfolio readiness, and delivery gating affect review-device and export fitness.
- Story Director: shot identity, manifest binding, review queues, and cross-work audits must preserve source order and human narrative authority.
- Sound Designer: voice-take selection, retake spans, score provenance, and no-score comparison directly affect audio review without allowing REEL to synthesize or approve sound.

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Generation plans bind only prompt/input hashes and expected picture shape; materialization verifies exact output hashes, bytes, and dimensions without executing a provider. | P3 | generation evidence | Keep prompt text and provider parameters outside portable receipts. |
| 2 | Picture planning distinguishes exact-byte reuse from recipe-equivalent regeneration and reports stale or missing work without rendering. | P3 | incremental planning | Preserve per-shot recipe keys and do not broaden reuse across a changed manifest hash. |
| 3 | Sprite coverage exposes exact, declared-fallback, and unresolved requests without guessing a pose. | P3 | selector coverage | Keep domain selector semantics in consuming repositories. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Timecoded findings are constrained to exact shot ranges and compile in deterministic severity/shot/time order. | P3 | repair queue | Retain absolute timeline validation so revisions cannot drift to another cut. |
| 2 | Promotion records reject skipped or reversed states and preserve prior-record hashes, making selection history append-only. | P3 | asset promotion | Treat selected and approved as ledger states, never as edit-lock or publication authority. |
| 3 | Review proxies require an explicit review profile and disclosure and can never become delivery-ready. | P3 | picture plan | Keep proxy disclosure in every downstream render-plan projection. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Materialization and cache checks bind exact dimensions to named output profiles, preventing silent shape drift. | P3 | output validation | Add new profiles explicitly rather than reinterpreting existing IDs. |
| 2 | Portfolio audit aggregates timing, generation, asset, preview, and delivery readiness while omitting local manifest paths. | P3 | production-state audit | Preserve generic blocker codes for unreadable or invalid entries. |
| 3 | Delivery readiness remains technical and is false for proxies even when every byte is reusable. | P3 | authority boundary | Require separate human and rights gates in consumer release workflows. |

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Every operation binds the validated production-manifest hash, so stale story or shot revisions cannot silently reuse prior evidence. | P3 | manifest binding | Keep the full manifest hash in cross-consumer receipts. |
| 2 | Repair queues preserve shot IDs and timed spans but do not contain or rewrite creative rationale. | P3 | review findings | Keep private reasons in the human review system that owns them. |
| 3 | Portfolio reports use explicit index IDs instead of discovering or reordering works implicitly. | P3 | portfolio audit | Require index changes to be reviewed as production-scope changes. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Voice takes bind cue/take IDs, exact audio hashes, exact cue spans, and the voice-plan hash; selection is a separate explicit record. | P3 | voice ledger | Continue to reject selection of a rejected take. |
| 2 | The retake queue contains only rejected or missing cue spans; available unselected takes remain awaiting human selection. | P3 | surgical retakes | Do not infer voice approval from technical selection or hash verification. |
| 3 | Music provenance supports an explicit no-score variant and rejects score/no-score claims unless both exact audio hashes exist. | P3 | music provenance | Keep source/license classification separate from rights approval and human listening. |

## Synthesis

Roles reviewed: 5  
P1 blockers: 0 | P2 issues: 0 | P3 notes: 15

Verdict: **APPROVED-WITH-CONDITIONS**

Cross-role consensus: the new surfaces are additive technical contracts. They
provide deterministic evidence and queues while leaving provider execution,
creative judgment, rights approval, principal approval, publication, and
release decisions to explicit human owners.

Conditions:

1. Consumer workflows must retain separate human review and rights gates after
   any REEL technical readiness result.
2. Portable receipts and reports must remain path-free and must not gain prompt
   text, credentials, provider secrets, private reasons, or local cache roots.
3. Proxy review artifacts must retain their disclosure and must never be
   promoted to delivery by a technical cache hit alone.

## Human authority boundary

`candidate`, `selected`, and `approved` describe only asset-ledger progression.
`reviewed` describes only a declared human-review status. `delivery_ready`
describes only technical picture readiness for a delivery-purpose profile.
None of these fields grants creative approval, performer or principal approval,
rights clearance, permission to publish, or permission to release.
