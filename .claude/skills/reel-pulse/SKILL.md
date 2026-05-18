# REEL Pulse

Use this skill for one REEL execution slice.

## Workflow

1. Read `CLAUDE.md` and the active wave `WAVE.md`.
2. Make the smallest complete change to briefs, formats, manifests, panels,
   renderer experiments, or docs.
3. Keep large generated media out of git unless a later policy explicitly allows
   a small fixture.
4. Update the pulse record with changes and validation.
5. Run the validation commands named by the pulse before committing.

## Default validation

```powershell
git grep -n "REEL" -- README.md PRODUCT_PLAN.md context\waves\PHASES.md
git diff --check
```
