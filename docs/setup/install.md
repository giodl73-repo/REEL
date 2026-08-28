# Install REEL

REEL is a versioned Rust executable. Consumer repositories exchange YAML/JSON,
caption, render, and provenance artifacts; they do not import REEL internals.
CLI v0.2.17 retains `reel.manifest.v0.2` and the separately versioned series,
episode-packet, cue-import, and continuity contracts. It adds smooth subpixel
motion as the default renderer path plus explicit v0.2.1 legacy reproduction,
and adds the separate cross-scene voice-consistency preflight contract.

For local development:

```powershell
cargo install --path . --locked
reel --version
```

For a tagged release, open the
[GitHub Releases page](https://github.com/giodl73-repo/REEL/releases/latest)
and download the binary for the target system:

- Windows: `reel.exe`
- Linux x86-64: `reel`

The binary is self-contained and does not require Rust or a repository clone.
On Windows, place `reel.exe` in a directory listed in `PATH`. On Linux, mark the
download executable with `chmod +x reel`, move it to a directory in `PATH`, and
run `reel --version` to verify the installation.

Record `reel --version` in conform/render lineage. Rendering additionally
requires FFmpeg and ffprobe. Windows currently invokes them through WSL; Linux
invokes them directly.

Release tags use `v<crate-version>`. CI builds and tests both operating systems
before release binaries are attached.
