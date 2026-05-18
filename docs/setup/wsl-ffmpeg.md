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

## What the smoke proves

- WSL2 can run the REEL renderer path.
- FFmpeg can encode an MP4 from manifest-derived text cards and silence.
- REEL manifests can drive renderer inputs without a provider-specific schema.

## What the smoke does not prove

- Final art direction.
- Remotion integration.
- Blender/3D rendering.
- AI-video provider continuity.
