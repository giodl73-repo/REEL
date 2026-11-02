# Pulse 06 - PITFALL Doctrine Integration

## Scope

Add REEL's PITFALL doctrine index for recurring moving-image production failure
classes, then tie it back to README discovery, role review, production
operations, receipt validation, and portfolio reuse.

## Findings

- A valid render, artifact check, receipt, comparison layout, or readiness audit
  must not become creative approval, rights clearance, principal approval, or
  release approval.
- Provider-specific SDKs, credentials, endpoints, models, renderer binaries, or
  provider-only fields must not enter the manifest contract before an accepted
  adapter work package needs them.
- A polished trailer, cinematic, explainer, or production package must not
  rewrite source canon, rights boundaries, factual claims, or source-repo
  release authority.
- Portable receipts and readiness reports must not leak paths, cache roots,
  filenames, prompts, credentials, provider secrets, private review reasons, or
  unapproved source identities.
- Untimed manifests, infeasible layouts, stale lineage, missing assets, or
  render-environment failures must not produce partial media that looks usable.

## Integration

- `.pitfall/PITFALL.md` indexes REEL principles, invariants, and pitfalls.
- `.pitfall/reel-principles.md` maps durable decision rules to README,
  PRODUCT_PLAN, CLAUDE, production docs, setup docs, and roles.
- `.pitfall/reel-invariants.md` records manifest-version, hash-lineage,
  private-state, release-approval, and render-binary properties.
- `.pitfall/reel-pitfalls.md` records mitigated failure classes for future
  manifest, render, adapter, review, and portfolio handoff work.

## Validation

Completed before commit:

```powershell
cargo test --quiet
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo run --quiet -- validate manifests\fixtures\two-speaker-untimed\planning.yaml --output json
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
git diff --check
```

Render-adapter validation was attempted with:

```powershell
cargo run --quiet -- smoke
```

It could not complete on this host because the WSL command path reported
`ffmpeg: command not found`. This preserves `REEL-PF-05`: missing render
environment evidence blocks render-smoke claims rather than producing partial
media or implying release readiness.
