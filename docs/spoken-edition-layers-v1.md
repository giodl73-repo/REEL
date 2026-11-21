# Music and Sonic effects on a spoken edition

**Status:** M07 implementation on merged M06 `cef69f7`, [PR #384](https://github.com/giodl73-repo/REEL/pull/384).
Qualification is recorded in the [checklist review](reviews/2026-11-21-m07-spoken-layers-review.md).
This adds no music generation,
separation, arrangement, pictures, automatic source selection or release.

## Reuse and boundary

M04 independently checks the original spoken candidate during preparation. Its
sealed parent witness and dialogue WAV stay immutable. Each subsequent produce
or check rereads the actual consumed D/M/E bytes and exact Sonic control pins;
it does not recheck unrelated parent editions. Existing `sonic_assets::resolve`
governs exact bed/effect selection;
the adapter supplies explicit verified input paths rather than discovering a
cache or falling back to catalog-relative files. Existing `audio_mix::compile`
owns event gains, hold automation, fades and D/M/E stems. The existing bounded
album process runner owns child timeout/reaping. No second mixer or process
engine is introduced.

An in-memory audio projection supplies only audio fields to that existing mixer;
it is not serialized or represented as a valid picture-production manifest.
There are no placeholder scenes, shots or image reads.

The v1 adapter consumes parent packages checked under the current REEL producer
identity. M05 retains the original older contexts independently.

## Versioned request

`reel.spoken-layers.v1` binds one edition's compiled and execution recipe,
exact Sonic catalog/request FileRefs, a full-source music bed, one full-source
effect, explicit overlap permission and authored music ducking. Sonic bindings
are exactly `bed` and `effect`; their existing consumer hash binds the canonical
layer intent (edition, parent recipes and policies), excluding catalog/request
pointers to avoid a circular hash. The final mix identity includes both exact
control pins, selected audio identities, runtime and resolved placement.

An anchor names selected performance ID, exact source SHA-256, marker ID and
start/end edge. Resolve against a freshly compiled original edition, never a
timestamp borrowed from another language or a mutable latest selection. Missing,
unselected, changed or out-of-range anchors refuse.

| Layer | Policy |
| --- | --- |
| D | Original checked dialogue, unity gain; no retiming or normalization |
| M | Full bed starts at zero; base gain −60..0 dB in millidecibel steps, two semantic anchors define a hold-duck window, additional reduction −60..0 dB in the same steps; explicit entrance/exit fades |
| E | Full effect begins at its semantic anchor; gain −60..0 dB in millidecibel steps and explicit fades |
| Tail | Retain the complete selected source. Refuse a source extending beyond the edition instead of truncating or extending it |
| Overlap | Separate explicit permission for overlap with dialogue and between bed/effect. Undeclared overlap refuses |

Authored hold ducking uses the existing gain-automation contract. It is not an
adaptive compressor or an intelligibility decision. Adaptive ducking and dynamic
EQ remain available through their existing production contracts; they are not
silently applied here. A real mix still needs listening for masking.

## Clock, bounds and independent checks

This adapter declares the existing mixer's **one-millisecond** event clock:
48 kHz stereo PCM24 only, and every placement, duration, fade and duck boundary
must be an exact multiple of 48 samples. Reject finer anchors; do not round them.
Input audio is explicitly framed into 48-sample blocks before existing hold
automation, with floating format established before an integer `1/1000`
timebase. This prevents floating timestamp undershoot from delaying a boundary
by one block. The duck window obeys `0 < start < end < bed_length`.
This does not claim
sample-granular automation support for arbitrary inputs.

At most 720,000 output samples, 16 MiB distinct source WAV bytes, two selected
Sonic sources, 256 KiB each catalog/request, 64 MiB final output and 128 MiB
workspace. Execution has a 30-second deadline, narrowed by the caller; zero
refuses before writes/spawn. Use a declared absolute FFmpeg executable with its
exact FileRef pin (at most 256 MiB). Runtime pinning is not a complete environment
or sandbox claim. `bind_runtime` reuses the existing `BoundTool` guard on Windows;
other platforms recheck a declared cooperative runtime without claiming native
protection. Enrollment happens before the bounded media fixture; its exact-byte
read is separately capped. The 30-second execution checks are cooperative around
trusted synchronous I/O; only the subprocess is actively killed/reaped at its
deadline. Child exit/reaping precedes source guard release. No downloads,
cache initialization, hydration, daemons or production media are used by tests.

Four lossless WAVs are published: dialogue, music, effects and mix. Private
`receipt.json` is the last no-clobber commit marker. Staging is test/operation
owned; failure preserves foreign files and earlier versions.

Native providers keep their protected source/runtime guards alive through the
borrowed adapter. Local filesystem providers are cooperative and are rechecked
before/after rendering; this is not native admission or protection parity.
Concurrent writers to the newly reserved destination are unsupported.

The checker reopens actual source and output PCM and independently evaluates
gain, hold ducking, fades, placement, silence and D+M+E. It does not call FFmpeg or
the producer. D must be bit-exact. M/E tolerance is two PCM24 integer units and
mix tolerance four, accommodating the existing FFmpeg floating-point graph's
conversion; samples outside source support must be exactly zero. Peak rail is
7,476,354 (existing spoken profile, full scale 2^23), with no clipping/limiter.
No loudness, acoustic-word fidelity, creative selection or authority is inferred.

## Engineering oracle fixed before execution

Use the original four-edition 48 kHz signal fixture. Add an `emphasis` phrase
marker inside closing at [4800,9600) EN and [9600,14400) ES. A separate opening
marker defines ducking [2400,139200) EN / [4800,163200) ES. These are controlled
semantic clocks on signals, not audible words. The bed has 216,000 samples and
the effect 4,800; both use distinct deterministic stereo values. Base bed gain
−12 dB, authored duck reduction −6 dB, effect gain −6 dB, fades 50 ms.

| Edition | Effect start / end samples | Duck interval |
| --- | --- | --- |
| EN executive | 160800 / 165600 | [2400,139200) |
| EN technical | 369600 / 374400 | [2400,139200) |
| ES executive | 194400 / 199200 | [4800,163200) |
| ES technical | 432000 / 436800 | [4800,163200) |

`spoken_programme_media` adds `layers::` cases for full-stream independently
calculated PCM/stems; language-local placement; bed v2 changes mix identity but
not dialogue recipes/WAVs; rehashed output/receipt tampering; stale marker/source;
missing scope; invalid gain/fades/sub-ms anchors; undeclared overlap; source tail
overrun; timeout, interruption and no overwrite. A real FFmpeg qualification must
run explicitly on Windows and Linux; an absent runtime is not a passing skip.
Run core/clock/native regressions, all-feature lint, formatting and diff checks.
Sound, Editor and Rights are required checklist lenses. Listening remains open.

```powershell
cargo test --locked --test spoken_programme_media layers:: -j2 -- --ignored
cargo test --locked --features tig-history --test spoken_programme_history --test spoken_programme_media --test spoken_programme --test cue_relative --test audio_event_clocks --test v0213_cli --test sonic_assets_v0318 -j2
cargo clippy --all-targets --all-features --locked -j2 -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The four external-runtime cases are intentionally ignored by ordinary unit runs
and explicitly invoked by **both** CI media lanes. An ignored listing is not
qualification; the explicit command must execute all four successfully.
