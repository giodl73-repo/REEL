# REEL Product Plan

## Thesis

REEL is the Design Labs home for making movies and videos across the portfolio.
It should let us move from an idea to a reviewable production package: a brief,
format grammar, script, shot list, edit manifest, generation prompts, review
notes, and eventually rendered outputs.

The repo starts as a design lab rather than a renderer because the hard problem
is not only producing frames. The hard problem is preserving intent across
story, timing, sound, captions, aspect ratios, model prompts, edits, and export
targets.

## Users

- Portfolio maintainers making product demos, launch clips, and explainers.
- Game/design repos needing trailers, cinematics, and animated briefs.
- Knowledge/applied repos needing short visual explanations of systems.
- Future automation agents that turn REEL manifests into rendered media.

## First capabilities

1. Define video briefs with audience, platform, duration, format, references,
   constraints, and acceptance criteria.
2. Convert briefs into scripts, scene beats, shot lists, and edit decision notes.
3. Select a format and animation style before choosing a renderer.
4. Capture generation manifests without locking into a provider.
5. Review works through a panel of filmmaker/editor/animation/audio/platform lenses.
6. Record innovations and rubric amendments as repeated production gaps appear.

## Games Design scenario videos

REEL should be able to turn Games Design scenario packs into multiple video
forms without changing the scenario truth owned by the game repo.

Initial targets:

- 30-90 second trailers for BANISH, QUEST, TIGRIS, AMAZE, and future games.
- Storyboard animatics that prove scene order, pacing, and narration before
  expensive rendering.
- Style variants of the same scenario, such as pixel-art, isometric-game,
  illustrated-2d, 3d-previs, or cinematic-ai.
- Mobile/social cuts with captions and fast hooks.

The production package should preserve:

- source scenario reference,
- selected format and style,
- scene beats and shot list,
- character/location continuity notes,
- audio and caption plan,
- renderer/provider assumptions,
- export targets.

The first manifest contract is intentionally renderer-neutral. It should be
specific enough for a human or agent to choose FFmpeg, Remotion, Blender, browser
capture, or an AI-video provider later, but it should not encode provider-only
fields as required foundation metadata.

## Non-goals

- No large media binaries in git.
- No default dependency on a single AI video provider.
- No native iOS or desktop editor in the foundation wave.
- No product-to-product dependency from sibling repos into REEL.
- No direct edits to game scenario canon from inside REEL.

## Dependency-chain placement

REEL is a Design Labs product/lab repo. It is not a shared primitive. It may later
consume:

- ROLES for repo-local review panels.
- PROOF for Markdown/report validation.
- CROP/PEBBLE/FLETCH for portable production packets.
- SCENE, PROSE, and SCORE as methodology siblings, not runtime dependencies.

## Validation contract

The foundation wave is documentation-first:

```powershell
git grep -n "REEL" -- README.md PRODUCT_PLAN.md context\waves\PHASES.md
git diff --check
```

Later implementation waves should add renderer-specific validation only after a
renderer is selected through `reel-research`.
