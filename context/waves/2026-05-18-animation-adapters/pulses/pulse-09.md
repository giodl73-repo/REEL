# Pulse 09: Adapter metadata determinism

## Outcome

Hardened optional adapter metadata for deterministic planning.

## Changes

- Preserved `renderer_assumptions.adapters` declaration order in adapter-plan
  and review-pack summaries.
- Kept undeclared known adapters visible after declared adapters.
- Rejected duplicate optional adapter ids during manifest validation.
- Documented the uniqueness and ordering contract.

## Validation

```powershell
cargo test --quiet
cargo run --quiet -- adapters
cargo run --quiet -- adapter-plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- validate manifests\templates\scenario-video.yaml
cargo run --quiet -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run --quiet -- smoke
cargo run --quiet -- review-all works
git diff --check
```
