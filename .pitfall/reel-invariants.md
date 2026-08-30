# REEL Invariants

These entries summarize properties that must remain true for REEL manifests,
derivatives, receipts, adapter boundaries, and review records.

## REEL-I-01: Manifest Contract Changes Are Additive And Versioned

**Status:** VERIFIED

**Claim:** `reel.manifest.v0.2` remains stable while new behavior is added
through separately versioned sidecars, artifacts, operations contracts, or CLI
behavior.

**Why it matters:** Portfolio consumers need validation behavior they can pin
without surprise migrations.

**Enforcement:** Requirement matrices and role reviews repeatedly record
manifest stability, no migration, and additive sidecar boundaries.

**Evidence:** `docs/production-manifest-v0.2.md`,
`docs/reviews/2026-08-06-v0.2.14-requirement-matrix.md`, `README.md`, and
`PRODUCT_PLAN.md`.

## REEL-I-02: Derivatives Preserve Hash Lineage

**Status:** VERIFIED

**Claim:** Conformed manifests, captions, review packets, receipts, comparison
outputs, production packages, and operations reports bind inputs, outputs,
tool versions, hashes, timing, and lineage without mutating their parents.

**Why it matters:** Video work creates many variants; hidden mutation makes
reviews stale and invalidates downstream comparisons.

**Enforcement:** Atomic no-clobber writes, path-free receipts, lineage reports,
review indexes, exact packet binding, and tamper rejection preserve history.

**Evidence:** `docs/production-manifest-v0.2.md`,
`docs/render-lineage-v0.2.5.md`,
`docs/selection-lock-and-audio-cache-v0.2.21.md`, and
`docs/production-operations-v0.2.36.md`.

## REEL-I-03: Portable Reports Omit Private Local State

**Status:** VERIFIED

**Claim:** Shareable receipts and portable operations reports omit local paths,
cache roots, credentials, provider secrets, private reasons, prompt text, and
unapproved source identities.

**Why it matters:** Video production often touches private assets, voices,
prompts, paths, and review reasoning that should not leak through evidence
packets.

**Enforcement:** Path-free receipt schemas, strict JSON, provider-package
gates, local/private artifact separation, and report privacy reviews protect
the boundary.

**Evidence:** `docs/privacy-safe-receipt-v0.2.6.md`,
`docs/receipt-check-v0.2.7.md`,
`docs/comparison-composer-v0.2.12.md`, and
`docs/production-operations-v0.2.36.md`.

## REEL-I-04: Technical Readiness Does Not Promote Release

**Status:** VERIFIED

**Claim:** Delivery readiness, cache readiness, asset promotion state, review
queue state, receipt validation, and artifact checks never imply creative,
principal, rights, publication, or release approval.

**Why it matters:** A valid render or selected asset can still be creatively
wrong, rights-unsafe, or unauthorized for publication.

**Enforcement:** Production operations set approval flags false, review records
separate advisory and final authority, and role reviews preserve human gates.

**Evidence:** `docs/review-decisions-v0.2.13.md`,
`docs/production-operations-v0.2.36.md`, `.roles/ROLE.md`, and `README.md`.

## REEL-I-05: Large Render Outputs Stay Out Of Git By Default

**Status:** VERIFIED

**Claim:** REEL does not store large binary renders in git by default; source
manifests, artifacts, receipts, docs, and reconstruction instructions carry
reviewable evidence instead.

**Why it matters:** Render binaries can bloat the repo, hide provenance, and
make review history hard to inspect.

**Enforcement:** README non-goals, CLI artifact manifests, receipt checks,
path-free reports, and setup docs keep renders as generated/local artifacts.

**Evidence:** `README.md`, `CLAUDE.md`, `docs/setup/install.md`, and
`docs/production-operations-v0.2.36.md`.
