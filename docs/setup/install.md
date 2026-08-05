# Install REEL

REEL is a versioned Rust executable. Consumer repositories exchange YAML/JSON,
caption, render, and provenance artifacts; they do not import REEL internals.
CLI v0.2.2 retains `reel.manifest.v0.2` and the separately versioned series,
episode-packet, cue-import, and continuity contracts. It adds smooth subpixel
motion as the default renderer path plus explicit v0.2.1 legacy reproduction.

For local development:

```powershell
cargo install --path . --locked
reel --version
```

For a tagged release, download the Windows or Linux binary from the GitHub
release and place it on `PATH`. Record `reel --version` in conform/render
lineage. Rendering additionally requires FFmpeg and ffprobe. Windows currently
invokes them through WSL; Linux invokes them directly.

Release tags use `v<crate-version>`. CI builds and tests both operating systems
before release binaries are attached.
