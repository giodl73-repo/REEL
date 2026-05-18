# Wave: Foundation studio

## Goal

Create REEL as the portfolio's Design Labs repo for movies and videos, with the
minimum durable structure needed to start making video packages without locking
into a renderer too early.

## Pulse table

| Pulse | Title | Status | Outcome |
|------:|---|---|---|
| 01 | Repo scaffold and intake | done | Created REEL docs, skills, initial rubric, validation contract, and TRACKER intake. |
| 02 | Animation style grammar and roles review | done | Added animation style catalog, Games Design scenario video flow, `.roles` panel, and foundation plan review. |
| 03 | Production manifest sketch | done | Defined the renderer-neutral YAML manifest contract and starter template for scenario videos. |
| 04 | Renderer research | done | Selected WSL2 Ubuntu + FFmpeg as the baseline path, Remotion as the first animation prototype boundary, and Blender/AI-video as deferred style-specific paths. |
| 05 | WSL FFmpeg smoke render | done | Added a WSL setup guide and manifest-fed FFmpeg smoke renderer that writes gitignored MP4 drafts. |
| 06 | First original video package | done | Created the Ash Vale trailer package from BANISH scenario evidence and rendered its first FFmpeg smoke draft. |
| 07 | FFmpeg shot-card draft | done | Added a manifest-driven shot-card renderer that turns each shot into a timed draft card sequence. |
| 08 | Shot-card export variants | done | Added 16:9 YouTube and 9:16 iPhone/social shot-card exports from the same manifest. |
| 09 | Manifest validation gate | done | Added a WSL-friendly manifest validator for required fields, timeline consistency, and export/platform targets. |
| 10 | Manifest-driven export timing | done | Updated the shot-card renderer to validate first and derive aspect ratios and duration scaling from manifest exports. |

## Success criteria

- REEL has a clear movie/video product thesis and explicit non-goals.
- The REEL rubric and production vocabulary are documented.
- Repo-local wave, pulse, and research skills exist.
- Repo-local `.roles` panel reviews story, animation style, edit, sound, and platform fit.
- REEL can describe multiple animation styles for Games Design scenario videos before renderer selection.
- REEL has a renderer-neutral production manifest contract covering scenario source, format, style, scenes, shots, audio, captions, continuity, assumptions, and exports.
- REEL has a researched first renderer path that keeps Linux video tooling in WSL2 and avoids provider lock-in.
- REEL can produce a gitignored FFmpeg smoke MP4 from the starter scenario manifest in WSL2.
- REEL has a first original Games Design scenario video package with brief, script, shotlist, manifest, and panel review.
- REEL can render a gitignored shot-card MP4 that follows a work manifest's shot order and durations.
- REEL can render platform-specific shot-card drafts for YouTube/demo and iPhone/social review.
- REEL can validate manifest structure, timeline continuity, and export targets before rendering.
- REEL can derive draft export timing from manifest platform/export targets.
- Validation commands are named before renderer-specific tooling is chosen.
- TRACKER records REEL as a Design Labs repo and dependency intake candidate.
