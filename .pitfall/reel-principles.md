# REEL Principles

These entries summarize durable REEL decision rules for video production
contracts, review authority, provenance, renderer boundaries, and portfolio
reuse.

## REEL-P-01: Design Before Rendering

**Status:** ACTIVE

**Statement:** REEL owns typed manifests, timing, source references, audio,
captions, review packs, and render planning before expensive or provider-bound
rendering occurs.

**Rationale:** The hard problem is preserving intent across story, timing,
sound, captions, aspect ratios, prompts, edits, and export targets.

**Decision rule:** A work must name format, style, source scenario, manifest
contract, platform, timing lifecycle, and review surface before renderer
execution is treated as meaningful.

**Evidence:** `README.md`, `PRODUCT_PLAN.md`, `CLAUDE.md`, and
`docs/production-manifest-v0.2.md`.

## REEL-P-02: Source Canon Stays Upstream

**Status:** ACTIVE

**Statement:** Source repositories own scenario truth, rights decisions, and
release authority; REEL adapts and cites them through manifest references.

**Rationale:** Video packaging can accidentally rewrite a game, product, or
research claim if adaptation and source authority are not separated.

**Decision rule:** REEL may validate and plan a production package, but it does
not change upstream canon, approve rights, or publish on behalf of the source
repo.

**Evidence:** `README.md`, `PRODUCT_PLAN.md`, `CLAUDE.md`,
`docs/reviews/foundation-plan-review.md`, and
`docs/production-operations-v0.2.36.md`.

## REEL-P-03: Receipts Prove Lineage, Not Approval

**Status:** ACTIVE

**Statement:** Receipts, artifacts, checks, queues, and readiness reports prove
technical facts and lineage; they do not imply creative, principal, rights,
publication, release, consent, or final-authority approval.

**Rationale:** Hash-bound evidence is persuasive and easy to overread as an
approval state.

**Decision rule:** Any technical report that can be shared or consumed by
automation must preserve explicit non-approval fields or omit approval entirely.

**Evidence:** `README.md`, `docs/privacy-safe-receipt-v0.2.6.md`,
`docs/review-decisions-v0.2.13.md`,
`docs/production-operations-v0.2.36.md`, and `.roles/ROLE.md`.

## REEL-P-04: Provider And Renderer Boundaries Stay External

**Status:** ACTIVE

**Statement:** REEL orchestrates FFmpeg and describes planned Remotion, Blender,
and AI-video adapters without rewriting renderers or selecting provider SDKs,
credentials, endpoints, or models as foundation dependencies.

**Rationale:** Renderer lock-in would make production doctrine harder to review
and would couple unrelated portfolio repos to a product-specific runtime.

**Decision rule:** Provider-specific execution requires an explicit adapter
contract and accepted work package; baseline reports stay provider-neutral and
path-free where portable.

**Evidence:** `README.md`, `PRODUCT_PLAN.md`, `docs/setup/install.md`, and
`docs/production-operations-v0.2.36.md`.

## REEL-P-05: Human Review Complements Automation

**Status:** ACTIVE

**Statement:** Automated validation, analysis, comparison, and queue commands
support human review but do not decide whether a video is good, truthful,
legible, accessible, or releasable.

**Rationale:** Rhythm, emotion, execution, and legibility combine measurable
facts with taste, audience, device, rights, and principal judgment.

**Decision rule:** Role reviews, findings, dissent, and final-authority records
remain separate from CLI checks and render receipts.

**Evidence:** `README.md`, `.roles/ROLE.md`,
`docs/reviews/2026-08-06-v0.2.13-review-decision-role-review.md`, and
`docs/production-operations-v0.2.36.md`.
