# Build and verify a local REEL workflow package

Use PowerShell 7 on Windows with Git and the Rust toolchain for the REEL
checkout. Start from the clean commit you want to distribute. The output's
parent directory must already exist. Choose an output name that is unused.

```powershell
pwsh -NoProfile -File tools/package-workflow.ps1 -OutputDirectory F:/releases/reel-workflow
```

The script builds with the checked-in Cargo lockfiles. It publishes the seven
executables and `manifest.json` together. The manifest records the exact REEL
commit, Rust compiler version, file sizes and SHA-256 digests. A failed build
can leave its staging directory beside the requested output.

Run these commands from the completed bundle to inspect the available paths.

```powershell
./reel-workflow.exe capabilities
./reel-workflow-native.exe capabilities
./reel-workflow-http.exe capabilities
```

Use `reel-workflow` to check plans and export or capture manual work.
`reel-workflow-http` submits explicitly applied requests and can reconcile an
existing request without resending it. `reel-workflow-native` handles managed
operations. The bundle also includes `reel`, `reel-tig-job` and
`reel-tig-speech`. Use [`reel-episode-captions`](episode-captions-v1.md) to
assemble SRT and VTT from exact native conform inputs. Check the target workflow's pinned native tools, media
dependencies and owned profiles separately before running a production job.

Verify an existing bundle against the source commit you intended to build.
Use `-Smoke` on a bundle you built locally to also run `--help` on each
executable after all hashes match. The verifier also accepts earlier six-tool bundles.

```powershell
pwsh -NoProfile -File tools/verify-workflow-package.ps1 -Bundle F:/releases/reel-workflow -SourceCommit (git rev-parse HEAD) -Smoke
```
