# Matched native Motioncraft comparisons

`reel-motioncraft-compare render <request.json> <new-package>` creates still,
directed and reduced-motion scene treatments from one exact directed job.
It is general native scene tooling, not limited to synthetic studies.

```json
{
  "schema": "reel.motioncraft-comparison-request.v1",
  "source_job": {
    "path": "selected/job.json",
    "sha256": "REPLACE_WITH_EXACT_SOURCE_JOB_SHA256",
    "bytes": 1234
  },
  "asset_root": "selected",
  "engine_commit": "REPLACE_WITH_FULL_PRODUCER_COMMIT"
}
```

Replace the illustrative hash, byte count and commit before use. Paths resolve
within the request's parent directory. Source job and asset locators are verified
before rendering. Supply the actual producer source commit; this field is a
caller declaration, while the running producer executable hash is measured.
For authoring, compile the selected scene first, then bind its directed job.
For an existing episode, bind its explicitly upgraded immutable job. The tool
does not choose new cels, takes, event IDs or creative defaults.

The source must contain a non-reduced phased camera. Still removes that camera;
reduced recompiles its direction with reduced motion against the same native
spans. Unchanged pictures, audio, captions, effect layers, geometry and clock
remain shared. Legacy cameras, crops, motion groups and post-compose cameras
require explicit migration and reject here. Removing a camera from a selected
source creates an audition, not a replacement of the accepted revision.

The package retains original job bytes, portable jobs and their exact contract,
production manifest and selected asset dependencies. Every treatment renders
through the native scene engine and checks its delivery receipt. All four native
stems must match byte-for-byte. `comparison.json` lists changed picture attachment
IDs, frame/sample clocks, exact variants, indexed frames, tool identity and the
complete file inventory. Hashes and native sample/frame indices are authoritative;
the browser's synchronized playback is a review aid.

Open `playback.html` through a local static HTTP server. Normal speed plays the
directed variant's native review audio; other variants are muted. Quarter speed
is silent diagnostic playback. No new captions or disclosure text are burned
into reusable cels or clean masters.

`reel-motioncraft-compare check <package>` verifies every inventoried byte,
regenerates treatment expectations from the source, rechecks native receipts,
reproduces cadence metrics and compares indexed PNG pixels against exact native
frame decoding. Moving the package preserves its relative locators. Tampering,
missing dependencies, source changes and altered variant claims reject.

`native_clock_verified` does not imply that motion quality passed. Each cadence
report and `all_camera_cadence_passed` retain the actual result; layered jobs may
need separate temporal validation. A produced package never grants creative
approval or publication permission. Evidence extraction is bounded to 256 frames
and 512 MiB of RGB pixels per scene; qualify a full episode scene by scene.

Before a CAIMITOS handoff, ingest retained files into the canonical cache and
commit hash/byte/provenance and hydration records. Worktree-local copies alone
are insufficient authority. A clean cache consumer must run the package checker.
