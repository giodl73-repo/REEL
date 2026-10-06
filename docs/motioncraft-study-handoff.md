> Current status: actual CAIMITOS adoption, corrected r4 package, canonical-cache authority and clean reproduction pass. See motioncraft-acceptance-audit.md. Earlier checkpoints below are historical.

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

Build `reel`, `reel-scene-authoring`, `reel-scene-build` and
`reel-scene-cadence` from this checkout.
The Python helpers require Pillow. Supply already generated landscape/portrait
study directories with all three native renders, passing hash-bound reports in
`cadence-r1`, and corrected caption sets in `review-comparison-v2`. The package
and clean workspace must not exist. Run scene cadence after each native render;
the report belongs outside its render receipt directory.

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
`target/motioncraft-clean-portrait-r1/reproduction-check.json` now supplies
the equivalent portrait proof: six exact compiled files, four exact WAV files,
450 decoded picture frames and 44 indexed review samples match the package.

Three transport tests cover tampered bytes, parent traversal and clean
new-only hydration. Three package tests cover legacy inventory behavior,
new source/audio component verification and approval separation.
The binary artifacts remain outside Git. Preserve this handoff before cleaning
`target`; these development paths are retained checkpoints, not a deployed
CAIMITOS package or a release.

For local visual comparison, copy `tools/motioncraft_playback.html` into the
parent of the default `motioncraft-demo-landscape` and
`motioncraft-demo-portrait` directories and serve that parent on loopback with
Python's HTTP server. The page offers normal/quarter playback, native/captioned
media and one audio focus. Its responsive layout supports phone-width
inspection. It does not submit data or produce production receipts. The review
session's temporary server was stopped and its browser viewport restored.

## Integrated package checkpoint r2

`target/motioncraft-handoff-r2` adds cadence reports, the caption-reserved
landscape/portrait derivatives and an inventoried `producer.json`. Its 840
components pass REEL package receipt/check. Package SHA256 is
`079f612edea69632804010a627c3aa6921369283dd5f969c0b31069d67a5913a`.
The earlier r1 package remains intact. This is a synthetic REEL integration
checkpoint; CAIMITOS adapter/cache adoption is still required.

All eight native renders were regenerated with the recorded REEL binaries.
`producer.json` pins media source commit
`fd0f9951504a857243da86d3d94dae12aeb80f8d`, all four binary SHA256/byte counts,
native render FFmpeg/FFprobe binary hashes and versions, Python/Pillow and
helper identities.
Its separate package-source commit
`768c70d3bdfba81869b116d0d00eff0a2f642b70` and helper hash identify the assembler.
The exact four Windows binaries are retained separately at
`target/motioncraft-runtime-fd0f995`; they are not shipped as media components.
Use that runtime snapshot for the local reproduction commands. A downstream
runtime also needs the documented FFmpeg/Python/Pillow dependencies. This does
not promise byte-identical recompilation of executables on another toolchain.

The six matched native study variants preserve every decoded frame and native
WAV byte from their earlier counterparts: 1,350 picture frames per profile.
Five compiled files are identical per variant; the compile receipt changes only
its compiler source hash, independently checked against the producing source.
The receipts retain that change rather than pretending an old producer made
the new media. Regeneration reports are inventoried under each base profile's
`review-comparison-v2`. Representative corrected caption frames were inspected
again. No new viewer or creative score is inferred from this matching proof.

The two caption derivatives preserve their parent job except for the explicit
layout setting. All native stems are exact; their 44 indexed samples keep the
band clear. They reserve picture space; caption text remains a separately owned
presentation layer. Their layout is an optional demonstration, not a rescore of
the original matched study.

Packing now rejects failed/stale cadence bindings before creating the package,
requires both caption profiles and a producer pin when they are included, and
preserves prior hydration/check receipts as inventory evidence instead of
replaying them into new input workspaces. Six transport tests cover these
failure cases and clean hydration. To assemble the integrated form:

```powershell
py tools/motioncraft_study_package.py pack <new-package> --landscape <base-landscape> --portrait <base-portrait> --caption-landscape <reserved-landscape> --caption-portrait <reserved-portrait> --producer <producer.json>
```

Each reserved root needs `caption-check.json` from
`tools/motioncraft_caption_check.py`, a passing `cadence-r1/revised.json`, its
compiled/revised render outputs and exact selected inputs. The base roots need
all three variants and their cadence reports. Generic inventory checks establish
integrity; verify scene receipts and execute the comparison tools before packing.

Hydration/reproduction accepts four keys: `landscape`, `portrait`,
`caption-landscape`, `caption-portrait`. It copies authored inputs and selected
cache objects; it creates a fresh receipt bound to the new package. It does not
copy compiled directories, renders or an old hydration receipt. Run the same
compile/build/verify sequence above for the selected key and a new workspace.
All four `target/motioncraft-handoff-r2-clean-<key>` workspaces passed the
retained-runtime compile/build/check and reproduction sequence. Each
`reproduction-check.json` binds the r2 package SHA and proves six exact compiled
files, four exact WAVs, all 450 decoded picture frames and 44 indexed review
samples. The command processes completed successfully; no compiled/rendered
outputs were copied during hydration.

The creative-selection gate stays pending and `release_ready` stays false.
These files are development evidence in `target`, not a canonical CAIMITOS cache
handoff or deployment. Preserve the package and runtime snapshot before cleaning
that directory.

## Follow-up runtime findings

The reused cadence analyzer invokes WSL FFmpeg separately from native rendering.
Refreshed reports in `cadence-r2` record actual backend/version. Assemble the
subsequent package with `--cadence-directory cadence-r2 --revision motioncraft-integrated-r2`
and the separate `motioncraft-package-producer-5645995.json` pin. Native media
retains its fd0f995 producer; refreshed analysis is produced by 5645995. Preserve
both runtime snapshots. The original r2 package and its four reproductions remain
intact; do not retag their older reports.

The legacy mixed-media smoke initially failed twice in WSL. Later shared-fixture
baseline/current commands, all 144 decoded frames and decoded audio matched,
and two unmodified current smoke runs passed. Earlier failures are retained with
no diagnosed cause and no production configuration change. See the acceptance
audit and `target/motioncraft-mixed-diagnostic-r1/render-comparison.json`.

### Verified r3 analyzer provenance checkpoint

`target/motioncraft-handoff-r3` passes the actual REEL package receipt/check:
840 components; package SHA256
`20760e61f00e9ebaa10b99d3d1e714bc3ba742c8cdaf1715f33184b75a514ccb`.
All eight refreshed cadence reports pass and match the separate actual analyzer
backend/version pin. 830 unchanged component hashes match r2; only the selected
cadence reports and producer/hydration metadata change. Four clean r3 input
hydrations verify every copied input and match the previously reproduced r2
inputs; no compiled/rendered outputs were copied. This refresh does not claim
four new rendered reproductions: r2 retains that rendering proof.

Actual CAIMITOS integration remains blocked: its animation-vfx entry check still
requires Asset Authority to provision or clear the dedicated worktree. The
bertica-session-handoff skill explicitly keeps such lanes gated until
administration establishes them. No CAIMITOS source, registry or canonical cache
was changed. Next required work is actual adapter/compiler adoption, future
episode and existing-version upgrade/rollback examples, the illustrated odd
sample canary, canonical authority/cache handoff and final integrated review.
The goal must remain incomplete until those requirements pass.
