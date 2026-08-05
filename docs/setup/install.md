# Install REEL

REEL is a versioned Rust executable. Consumer repositories exchange YAML/JSON,
caption, render, and provenance artifacts; they do not import REEL internals.

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
