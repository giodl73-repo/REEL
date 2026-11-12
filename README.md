# REEL

**Rust tooling for declarative media production.** Describe scenes, recordings,
timing, pictures and delivery rules in versioned files; compile them into jobs;
render through explicit tools; then independently check the resulting media.

REEL supports illustrated episodes, music videos, score and lyric presentations,
albums, print collections, trailers and explainers. It also provides cel, note
and lyric-underlay editors. Producing projects own their content and creative
decisions; REEL supplies reusable contracts, compilers, renderers and checks.

**Start here:** [Getting started](docs/getting-started.md) ·
[Use cases](docs/use-cases.md) · [Architecture](docs/architecture.md) ·
[Documentation](docs/README.md)

## Choose a workflow

| I want to… | Start with |
| --- | --- |
| Try REEL with a small checked-in example | [Validate and plan without rendering](docs/getting-started.md) |
| Build a scene from cels, cues and measured timing | [Native scene authoring](docs/scene-authoring-v1.md) and [delivery](docs/scene-delivery-v0.1.md) |
| Resolve versioned assets through Tig | [REEL with public Tig](docs/tig-public-integration.md) |
| Assemble an episode from retained scenes, openings and credits | [Episode workflow](docs/use-cases.md#assemble-an-illustrated-episode) and [managed jobs](docs/tig-episode-jobs.md) |
| Check a retained delivery candidate against its frozen source | [Retained delivery checker](docs/tig-retained-delivery-check.md) |
| Inspect exact compiled delivery controls without opening primary media | [Frozen delivery inspection](docs/tig-delivery-inspection.md) |
| Recover a missing original native dialogue/music/effect stem | [Exact native audio recovery](docs/tig-scene-audio-recovery.md) |
| Inspect or explicitly execute a private speech attempt | [Tig speech adapter](docs/tig-speech-generation.md) |
| Edit cels and timing visually | [Cel sorter](docs/cel-sorter-v1.md), [project mapping](docs/cel-sorter-project-v1.md) and [local editor host](docs/editor-host-v1.md) |
| Edit notation or align lyrics to notes | [Note workshop](docs/music-note-editor-v1.md) and [underlay workshop](docs/music-underlay-editor-v1.md) |
| Make a continuous album or album video | [Album mastering](docs/NATIVE-ALBUM-MASTER.md) and [album video](docs/NATIVE-ALBUM-VIDEO.md) |
| Produce a songbook, manuscript or bilingual PDF | [Native print collections](docs/native-print-collections.md) |
| Track output revisions delivered to different targets | [Delivery history workflow](docs/use-cases.md#track-a-delivery-over-time) |
| Check storage, dependencies and candidate evictions | [Asset-management workflow](docs/asset-manager-workflow-v1.md) and [Tig boundaries](docs/architecture.md#reel-and-tig) |
| Inspect a semantic dependency graph in a browser | [WASM workbench](https://giodl73-repo.github.io/REEL/assembly/) |

## Try it

From a source checkout, using a current stable Rust toolchain:

```sh
cargo build --locked --bin reel
cargo run --locked --bin reel -- validate manifests/fixtures/two-speaker-untimed/planning.yaml --output json
cargo run --locked --bin reel -- plan manifests/fixtures/two-speaker-untimed/planning.yaml --output json
```

These commands validate and plan a fixture without rendering or opening a cache.
The fixture is intentionally untimed; [conform it](docs/getting-started.md#conform-measured-timing)
before attempting delivery. See [installation](docs/setup/install.md) for binary
selection, optional features and platform-specific media dependencies.

## How the pieces fit

```text
Project: source text + pictures + recordings + definitions + policies
                              |
                  YAML/JSON controls and asset IDs
                              |
                  REEL validates and compiles
                              |
               explicit render job + input identities
                              |
                FFmpeg / native browser worker
                              |
               independent media checks + candidate
                              |
                  project review and delivery

Optional Tig: Git snapshot -> exact asset/byte resolution -> guarded inputs
              checked outputs -> retention and reviewed record proposals
```

A project can use explicit local files or the optional
[public Tig integration](docs/tig-public-integration.md). Tig owns generic asset
identity and storage services. REEL interprets media controls and checks frames,
samples, timing and composition. Project profiles specify locations, budgets,
definitions and delivery policy; the engines do not require a particular show,
drive layout or cloud provider.

## Available source and support boundaries

| Area | Available | Important boundary |
| --- | --- | --- |
| Planning and video | Manifest validation, timing conform, FFmpeg previews/delivery, captions, motion and independent checks | Legacy animatic commands use WSL on Windows; native delivery APIs use explicit native programs. |
| Native scenes and episodes | Sample/frame clocks, protected input bindings, retained component proofs and bounded managed jobs | Managed native runtime protection, strict Tig path guards and contained browser workers currently require Windows. |
| Music and print | Native music contracts, note/underlay proposals, album packets and bounded print collections | Engraving, measured transcription and provider workflows have their own documented limits; some older adapters use Python. |
| Authoring UI | Explicit cel, note and underlay exports; local Rust editor host; browser semantic workbench | A saved draft is a proposal. Reapply through native validation before rendering. The WASM workbench inspects graphs rather than playing or rendering media. |
| Storage integration | Exact input resolution, explicit publication/history hosts and scoped cache-accounting tools | Deletion requires enrolled policies, complete applicable coverage and verified custody; a successful render is insufficient. |
| Other renderer adapters | Remotion package handoff; declared Blender and AI-video adapter boundaries | These are planned execution integrations, not bundled production renderers. |

This is an evolving tooling repository. Use a guide's schema and feature flags,
and pin the actual producer/checker runtime for a project. Source on `main`, a
tagged binary and an installed project tool can represent different revisions.
[The roadmap](docs/tooling-roadmap.md) separates current source from proposals and
project adoption. [CI](https://github.com/giodl73-repo/REEL/actions/workflows/ci.yml)
qualifies source with synthetic fixtures; creative acceptance remains a project
decision.

## Editorial review

The REEL rubric is **Rhythm, Emotion, Execution, Legibility**. Format, audience,
sound, pacing and reading clarity guide the production review alongside technical
checks. [Formats](formats/), [styles](styles/), [scoring](scoring/) and
[review roles](.roles/ROLE.md) preserve that design-lab practice.

Technical validation, listening/visual review, selection, rights approval and
release are separate decisions. [Principles and pitfalls](.pitfall/PITFALL.md)
document the boundaries. Roles are checklists, not invented people or approvals.

## Repository map

| Location | Purpose |
| --- | --- |
| `src/` | Media engines, main CLI and optional Tig adapters |
| `crates/reel-assembly/` | Native semantic assembly/dependency contracts |
| `crates/reel-music/` | Native music contracts and editor tools |
| `crates/reel-print/` | Bounded print production |
| `crates/reel-assets/` | Existing asset accounting, editors and custody adapters |
| `browser/assembly/` | Browser/WASM semantic workbench |
| `manifests/` | Contracts, templates and synthetic fixtures |
| `docs/` | [Task guides and reference index](docs/README.md) |
| `vendor/tig/` | [Pinned, unmodified upstream Tig source](vendor/README.md) |
| `formats/`, `styles/`, `works/`, `scoring/`, `.roles/` | Creative design and review material |

Project media, machine profiles, generated reports and renders belong in the
producing project's asset system. Keep the reusable source repository small.

## Contributing and licensing

See [contributor checks](docs/contributing.md) before changing a contract or
adapter. Use generated fixture media and keep private operational evidence out of
public issues, commits and CI logs.

Software, scripts, tests, configuration and ordinary software documentation are
MIT-licensed. Original non-software content is CC BY-NC 4.0; commercial use of
that content requires separate written permission. Third-party material retains
its own terms. See [LICENSE](LICENSE).
