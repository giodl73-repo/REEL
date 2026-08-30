# REEL Pitfalls

These entries capture recurring moving-image production failure classes and map
them to REEL's existing controls.

## REEL-PF-01: Valid Render Becomes Release Approval

**Status:** MITIGATED

**Pattern:** A video passes manifest validation, render checks, receipt
verification, comparison layout, or production-state audit and is then treated
as creatively approved, rights-cleared, principal-approved, or publishable.

**Domain:** Animatic renders, production packages, asset promotion, review
queues, delivery readiness, portfolio audits, and customer handoffs.

**Detection difficulty:** Technical reports are hash-bound and authoritative
about files, so readers may infer broader approval from valid evidence.

**Structural solution:** Keep approval flags false, separate role findings from
final authority, require human release gates, and state non-approval in
portable reports.

**Evidence:** `docs/review-decisions-v0.2.13.md`,
`docs/production-operations-v0.2.36.md`, `.roles/ROLE.md`, and `README.md`.

## REEL-PF-02: Provider Lock-In Enters The Manifest

**Status:** MITIGATED

**Pattern:** A manifest or foundation contract requires one AI-video provider,
SDK, credential, endpoint, model, renderer binary, or provider-only field before
the style and adapter work justifies it.

**Domain:** Production manifests, generation plans, provider packages, Remotion
handoffs, Blender handoffs, AI-video packages, and portfolio reuse.

**Detection difficulty:** Provider details can feel like implementation
progress even when they narrow the contract too early.

**Structural solution:** Keep `reel.manifest.v0.2` renderer-neutral, describe
planned adapters separately, use hash-only provider-neutral plans, and require
explicit approval for outbound provider transfers.

**Evidence:** `PRODUCT_PLAN.md`, `README.md`,
`docs/production-manifest-v0.2.md`, and
`docs/production-operations-v0.2.36.md`.

## REEL-PF-03: Source Canon Is Rewritten By The Trailer

**Status:** MITIGATED

**Pattern:** A trailer, cinematic, explainer, or production package changes the
source repo's canon, factual claim, rights boundary, release posture, or
principal decision by embedding it in polished video form.

**Domain:** Games Design scenario videos, product demos, research explainers,
works manifests, review packs, and external handoffs.

**Detection difficulty:** Video is persuasive and condensed, so omissions,
adaptations, and dramatic emphasis can appear as source truth.

**Structural solution:** Require source scenario references, source ranges,
coverage checks, omissions bridges, upstream canon ownership, and review roles
before renderer work starts.

**Evidence:** `CLAUDE.md`, `README.md`,
`docs/production-manifest-v0.2.md`, and
`docs/reviews/foundation-plan-review.md`.

## REEL-PF-04: Private Production State Leaks Through Evidence

**Status:** MITIGATED

**Pattern:** Local paths, cache roots, filenames, prompts, credentials, provider
secrets, private reviewer reasons, voice/photo identities, or source asset IDs
leak into shareable receipts or portfolio readiness reports.

**Domain:** Animatic receipts, provider packages, comparison receipts,
production operations reports, review queues, materialization records, and
portfolio audits.

**Detection difficulty:** Local evidence is needed for debugging, and it is
easy to copy more detail than a recipient needs.

**Structural solution:** Separate local artifacts from portable receipts, use
strict path-free schemas, deny unknown fields, hash inputs, and omit private
reasoning from queue summaries.

**Evidence:** `docs/privacy-safe-receipt-v0.2.6.md`,
`docs/receipt-check-v0.2.7.md`,
`docs/comparison-composer-v0.2.12.md`, and
`docs/production-operations-v0.2.36.md`.

## REEL-PF-05: Timing Or Layout Failure Produces Partial Media

**Status:** MITIGATED

**Pattern:** Untimed manifests, infeasible slates, bad caption layout, stale
lineage, missing assets, or render-environment failures produce partial videos,
artifacts, or receipts that look usable.

**Domain:** Conform packets, caption export, animatic render, comparison
layout, motion checks, render doctor, artifact checks, and production packages.

**Detection difficulty:** Render pipelines can stage intermediate files before
all constraints are known.

**Structural solution:** Reject untimed delivery, preflight geometry and render
environment, publish packets atomically, refuse overwrite, and emit no final
video/artifact/receipt when checks fail.

**Evidence:** `docs/production-manifest-v0.2.md`,
`docs/animatic-verification-v0.2.3.md`,
`docs/render-environment-v0.2.4.md`, and
`docs/reviews/2026-08-06-v0.2.14-requirement-matrix.md`.
