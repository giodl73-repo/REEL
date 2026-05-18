# WSL FFmpeg setup

REEL runs first renderer tooling in Ubuntu on WSL2. Windows can orchestrate git
and editor work, but video tooling should run in Linux to avoid shell, path, and
codec friction.

## Check WSL

From PowerShell:

```powershell
wsl --status
wsl --list --verbose
```

Expected: Ubuntu exists and uses WSL version 2.

## Install/check FFmpeg

Inside Ubuntu:

```bash
sudo apt-get update
sudo apt-get install -y ffmpeg
ffmpeg -version
```

## Run the smoke renderer

From PowerShell:

```powershell
wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/smoke-render.sh'
```

The script reads `manifests/templates/scenario-video.yaml` and writes:

```text
renders/smoke/0001-example-scenario-video-smoke.mp4
```

`renders/` is gitignored. Commit the manifest, script, docs, and review notes,
not generated media.

## Run the shot-card renderer

Use the Rust CLI for validation and planning:

```powershell
cargo run -- validate works\0001-ash-vale-last-road-before-winter\manifest.yaml
cargo run -- plan works\0001-ash-vale-last-road-before-winter\manifest.yaml
```

Validate the work manifest before rendering:

```powershell
wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/validate-manifest.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'
```

Once a work has a full manifest, render a timed card sequence for every shot:

```powershell
wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml'
```

The default platform is `youtube-demo` and writes:

```text
renders/shot-cards/0001-ash-vale-last-road-before-winter-youtube-demo-shot-cards.mp4
```

Create a quick contact sheet for review:

```powershell
wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-contact-sheet.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml youtube-demo'
```

The contact sheet writes:

```text
renders/contact-sheets/0001-ash-vale-last-road-before-winter-youtube-demo-contact-sheet.png
```

Render every platform cut, contact sheet, and a summary review report:

```powershell
cargo run -- review-pack works\0001-ash-vale-last-road-before-winter\manifest.yaml
```

The review-pack report writes:

```text
renders/review-packs/0001-ash-vale-last-road-before-winter-review-pack.md
```

Render review packs for every work manifest:

```powershell
cargo run -- review-all works
```

The batch index writes:

```text
renders/review-packs/INDEX.md
```

Render the phone-first 9:16 cut with:

```powershell
wsl -- bash -lc 'cd /mnt/c/src/TRACKER/repos/design-labs/reel && bash scripts/render-shot-cards.sh works/0001-ash-vale-last-road-before-winter/manifest.yaml iphone-social'
```

The renderer validates the manifest before rendering and derives each platform's
aspect ratio and duration scale from the manifest `platforms` and `exports`
entries. The `iphone-social` platform compresses the shot durations to the
manifest's 45-second phone target and writes:

```text
renders/shot-cards/0001-ash-vale-last-road-before-winter-iphone-social-shot-cards.mp4
```

This is still a draft renderer: it proves order, duration, captions, narration,
and shot intent before we build a Remotion or final animation pass.

## What the smoke proves

- WSL2 can run the REEL renderer path.
- FFmpeg can encode an MP4 from manifest-derived text cards and silence.
- REEL manifests can drive renderer inputs without a provider-specific schema.
- REEL can render a timed shot-card draft from the manifest shot list.
- REEL can generate separate 16:9 and 9:16 draft exports from one manifest.
- REEL can validate manifest timing and export targets before rendering.
- REEL can derive shot-card export aspect ratios and duration scaling from the
  manifest instead of hard-coded platform timing.
- REEL can render scene-aware draft cards that include captions, visual prompts,
  camera notes, action, and narration for animation review.
- REEL can generate contact-sheet PNGs from draft cuts for fast shot-rhythm
  review.
- REEL can render a full gitignored review pack covering every manifest platform.
- REEL can batch-render review packs for all work manifests and write an index.
- REEL has a Rust CLI for validation, export planning, and adapter
  orchestration while FFmpeg remains an external renderer dependency.
- REEL writes review-pack reports and indexes in Rust; Bash is now only the
  FFmpeg media adapter path.

## What the smoke does not prove

- Final art direction.
- Remotion integration.
- Blender/3D rendering.
- AI-video provider continuity.
