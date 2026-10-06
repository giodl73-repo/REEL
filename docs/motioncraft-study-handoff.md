# Motioncraft study handoff

This handoff demonstrates transport and same-runtime reproduction of the
synthetic queue study through REEL's actual scene engine. It does not implement
CAIMITOS's adapter, establish its integration lane, or prove its odd-sample
canary. Those remain separate Motioncraft completion requirements.

The package uses `reel.production-package.v0.1`. Existing component kinds cover
authored `craft-plan`, compiled `choreography`/`production-manifest`, captions,
rendered video, department receipts and review evidence. Two additive kinds
cover `source-asset` and `render-audio`; consumers must use a REEL binary built
from the Motioncraft branch containing these variants. Older readers reject
the new kinds. The version string alone is insufficient to identify this
development capability; pin the producing Git commit and binary hash during
adoption. No motion-specific inventory schema is needed.

The transport helper records scoped binding IDs, stable logical IDs, hashes,
byte counts, `cache://sha256/` locators, selection state and synthetic provenance
in `hydration.json`. It transports existing selections rather than selecting
new revisions. Authoring files and selected cache objects hydrate into a new
workspace. Compiled files and renders remain reference artifacts in the package
and must be reproduced in the clean workspace.

## Command sequence

Build `reel`, `reel-scene-authoring` and `reel-scene-build` from this checkout.
The Python helpers require Pillow. Supply already generated landscape/portrait
study directories with all three native renders and corrected caption sets in
`review-comparison-v2`. The package and clean workspace must not exist.

```powershell
py tools/motioncraft_study_package.py pack <new-package> --landscape <landscape-study> --portrait <portrait-study>
reel production-package-receipt <new-package>/package.json --output-path <new-package>/package-receipt.json
reel production-package-check <new-package>/package-receipt.json <new-package>/package.json
py tools/motioncraft_study_package.py hydrate <new-package> landscape <new-clean-workspace>
reel-scene-authoring compile-delivery <new-clean-workspace> <new-clean-workspace>/compile-revised.json
reel-scene-build build <new-clean-workspace> compiled-revised/build.json --asset-root <new-clean-workspace> --output-dir <new-clean-workspace>/render-revised
py tools/motioncraft_study_package.py verify-reproduction <new-package> landscape <new-clean-workspace>
```

Use `portrait` and another clean directory to repeat the consumer proof for
that profile. No intermediate shot/sample timing is edited. The native render
includes scene-delivery checking before its successful scene-build receipt.

Hydration verifies all inventoried bytes before creating its destination,
rejects nonportable/escaping paths and refuses overwrite. Selected asset hashes
and byte counts must match the hydration manifest. The production-package
verifier independently verifies the inventory. These are integrity checks;
they do not infer rights, source authority or creative approval.

Reproduction verifies the exact six compiled JSON outputs, exact native
D/E/M/mix WAV bytes, all decoded picture frame hashes and indexed review PNG
pixels. Encoded video bytes may differ across runtimes; this checkpoint proves
the same runtime and does not claim arbitrary cross-runtime equality.

## Verified checkpoint

`target/motioncraft-handoff-r1` contains 627 inventoried components covering
both profiles and all six study renders. REEL's package receipt/check passes;
the creative-selection gate is pending and `release_ready` is false.
`target/motioncraft-clean-landscape-r1/reproduction-check.json` proves all six
compiled JSON files, four native audio files, 450 decoded picture frames and
the compiler-selected review samples match the packaged landscape reference.
The clean workspace received no compiled/rendered outputs during hydration.

Three transport tests cover tampered bytes, parent traversal and clean
new-only hydration. Three package tests cover legacy inventory behavior,
new source/audio component verification and approval separation.
The binary artifacts remain outside Git. Preserve this handoff before cleaning
`target`; these development paths are retained checkpoints, not a deployed
CAIMITOS package or a release.
