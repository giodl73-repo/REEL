# REEL phases

## Phase 1: Foundation studio

Goal: establish REEL as the Design Labs movie/video repo with a clear rubric,
production vocabulary, repo-local skills, and first validation contract.

Validation:

```powershell
git grep -n "REEL" -- README.md PRODUCT_PLAN.md context\waves\PHASES.md
git diff --check
```

## Phase 2: Production manifests

Goal: define machine-readable edit manifests for scripts, shots, audio, captions,
transitions, and export targets without committing to a renderer.

## Phase 3: Renderer research and pilot

Goal: choose the first renderer path through `reel-research`, then pilot one
short video package.

## Phase 4: Portfolio adoption

Goal: produce videos for selected downstream repos such as BANISH, ICELINES,
SCENE, SCORE, or public front-door demos.
