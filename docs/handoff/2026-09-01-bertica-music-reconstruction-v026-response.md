# REEL response — music-reconstruction foundation v0.2.26

Date: 2026-09-01
Consumer: private BERTICA soundtrack reconstruction workflow

## Request answered

BERTICA needs a reusable progression from song decomposition to minimal repair,
same-composition English performance, editable score recovery, and later
score-driven re-orchestration without copying private songs or project-specific
creative authority into REEL.

## REEL response

Slice A creates `reel-music` and the exact evidence boundary required before any
audio transformation:

```powershell
reel music-source-validate source.yaml --output json
reel music-neutral-plan source.yaml --output-path neutral.json --output json
reel music-neutral-check neutral.json source.yaml candidate.raw --output json
reel music-repair-plan repair.yaml --output json
```

The source and neutral contracts prove immutable raw-PCM identity. Repair plans
use integer half-open sample ranges, declare complete changed envelopes, and
lock every unaffected sample. Typed operations, external assets, authorities,
decision evidence, and required roles are validated without rendering or
network activity.

## Evidence

- Architecture: `docs/music-reconstruction-crate-proposal.md`
- Release contract: `docs/music-reconstruction-v0.2.26.md`
- Synthetic fixture: `manifests/fixtures/music-repair-foundation/`
- Contract gate: `signals/simulate/contract/reel-music-slice-a-contract-2026-09-01.md`
- Expanded implementation review:
  `signals/roles/check/reel-music-slice-a-v026-roles-check-2026-09-01.md`

The complete workspace passes on Windows and WSL/Linux. No BERTICA audio,
lyrics, paths, title, voice, identity, or creative decision entered REEL.

## Consumer boundary

BERTICA can later supply path-free, identity-free acceptance evidence against
its private `El guajiro pintor` pilot. Current v0.2.26 reports are local and
explicitly `shareable: false`; a redacted exchange receipt belongs to a later
slice.

## Next request

Slice B should render only the synthetic repair first: compile a canonical edit
decision list, execute it through the existing FFmpeg adapter, verify exact
outside-region signal identity, and bind acoustic seam/tail evidence before any
private song is used for acceptance.
