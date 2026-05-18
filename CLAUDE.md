# REEL — Movie and Video Design Lab

## Mission

REEL is a framework for designing, generating, editing, and evaluating moving
image works. We learn video formats by working in them: writing briefs,
structuring scenes, composing shots, building edit manifests, reviewing outputs,
and evolving the rubric through repeated production gaps.

The rubric is REEL: **R**hythm, **E**motion, **E**xecution, **L**egibility.

## Core vocabulary

- **Format** - a moving-image form with its own grammar, constraints, and
  viewing context.
- **Work** - a specific video artifact, either canonical or original.
- **Brief** - the assignment: audience, platform, duration, format, purpose, and
  constraints.
- **Scene** - a coherent unit of action or explanation.
- **Shot** - a camera, composition, motion, or generated visual unit.
- **Edit manifest** - the ordered production contract for shots, audio, captions,
  transitions, and exports.
- **Persona** - a filmmaker/editor/critic voice that reviews a work.
- **Lens** - a production perspective such as editor, cinematographer, sound, or
  platform/audience.
- **Innovation** - a structural technique or rubric gap discovered during review.
- **Amendment** - a rubric revision ratified from clustered innovations.

## Formats

Formats are documented in `formats/`. New formats are added when a work requires
a vocabulary not yet documented.

- `short-film`
- `trailer`
- `explainer`
- `social-clip`
- `game-cinematic`
- `animated-diagram`

## File naming conventions

- Works: `works/NNNN-<slug>/`
- Formats: `formats/<slug>.md`
- Personas: `personas/<slug>.md`
- Lenses: `personas/lenses/<slug>.md`
- Skills: `.claude/skills/reel-<name>/SKILL.md`
- Handoffs: `docs/handoff/YYYY-MM-DD-<slug>.md`

## Frontmatter contract

Every generated work file opens with YAML frontmatter:

```yaml
---
work: NNNN-slug
stage: brief|script|shotlist|manifest|panel|handoff
format: format-slug
author: persona-slug
rubric_version: v0.1
created: YYYY-MM-DD
updated: YYYY-MM-DD
sources: []
---
```

## Editorial rules

1. **Format first** - name the video grammar before judging the work.
2. **Device matters** - phone, desktop, theater, and social feeds are different
   viewing contracts.
3. **Sound is structural** - narration, music, silence, and effects are not
   afterthoughts.
4. **No binary sprawl** - keep renders out of git unless a later policy says
   otherwise.
5. **Forward-only rubric** - amendments apply after ratification; do not
   retroactively rescore completed works.
