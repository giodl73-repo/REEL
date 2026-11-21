# Bounded spoken-edition candidate production

**Status:** M03 implementation locally checked on merged M02 `cc099b4`.
Hosted qualification and merge remain pending; independent checking is M04.
See the [PR plan](media-expansion-pr-plan.md),
[compiler contract](spoken-edition-compilation-v1.md) and
[fixture specification](media-expansion-spoken-fixture.md).

```text
declaration + selected alignment metadata + exact compiler executable
                         │ recompile and compare canonical plan bytes
                         ▼
                  verified sample plan
                         │ read/hash complete selected WAVE sources
                         ▼
        exact PCM windows → explicit gain/fades → integer overlap sum
                         │ validate format, count, peak and limits
                         ▼
             new candidate directory + final receipt marker
                         │ M04 independently reopens and checks
                         ▼
           technical evidence, with human decisions still separate
```

## Explicit execution

```sh
reel spoken-edition-compile declaration.json --output plan.json
reel spoken-edition-produce plan.json --declaration declaration.json \
  --asset-root staged-inputs --output new-candidate
```

Use the same exact REEL executable for both commands. The producer rederives
the plan from the declaration and selected metadata under its own executable
hash. It accepts only the compiler's exact canonical bytes; editing a placement,
source, runtime or derived recipe hash does not bypass that comparison.
This is a current-runtime binding, not native runtime admission or historical
reopening. M05 owns retained-context integration.

`--processing policy.json` optionally selects explicit per-edition/performance
processing. Omitted operations mean gain 1/1 and no fade. No inferred leveling,
ducking, stretching, conversion or background music occurs.

```json
{
  "schema": "reel.spoken-processing.v1",
  "operations": [{
    "edition_id": "en-executive",
    "performance_id": "en-opening",
    "gain": {"numerator": 1, "denominator": 2},
    "fade_in_samples": 100,
    "fade_out_samples": 0
  }]
}
```

This example applies only when the first 100 samples are outside every protected
word, breath and tail. Unknown/duplicate targets, overlapping/out-of-bounds fades
and fades touching a protected region refuse. Gain is nonnegative, at most 8,
with a denominator from 1 through 1,000,000. Gain is signed integer division
toward zero, followed by the existing PCM-window linear envelope with the same
rounding. No gain/fade can overflow a signed 24-bit source sample. Overlap sums
use a wider integer accumulator and must meet the final profile peak limit.

## Input and output profiles

Only complete, bounded RIFF WAVE sources containing exact stereo 48,000 Hz
signed PCM24 are accepted. The producer reuses REEL's bounded WAVE parser and
PCM-window implementation; no external decoder, encoder or provider is launched.
This narrows the first producer to lossless native PCM. Other container/rate/
channel/codec conversion is rejected rather than silently normalized. Existing
song decoding and picture-bound scene production retain their contracts.

Every admitted source is read completely, including unselected suffixes, and
checked against its declared byte count, container hash and exact frame count.
Aliases with different path/hash/size pins remain separate admitted inputs.
Only sources consumed by the chosen editions are read; authority payloads are
not opened or granted by this producer.

| Limit | Engineering profile | Spoken profile |
| --- | --- | --- |
| Maximum edition duration | 720,000 samples (15 seconds) | 1,440,000 samples (30 seconds) |
| Aggregate selected source containers and PCM, each bounded separately | 16 MiB | 32 MiB |
| Final audio files | 64 MiB | 64 MiB |
| File workspace budget | 128 MiB | 128 MiB |
| Absolute output sample peak | 4,194,303 | 7,476,354 |
| Operation deadline | 30 seconds | 30 seconds |

The spoken peak is a conservative integer ceiling below −1 dBFS; it is not a
loudness, true-peak or intelligibility qualification. Metadata retains M02's
limits; plan and local receipt are each capped at 16 MiB, policy at 256 KiB.
The workspace bound covers staged/final files, not a process RSS guarantee.
Library hosts may narrow the deadline with `ProductionControl`; a wider deadline
is rejected. A zero-budget controlled test proves immediate timeout refusal.
Synchronous trusted input adapters must return; cancellation/deadline checks
surround their calls and run through bounded reads, PCM processing, mixing and
publication. This is not an operating-system sandbox for a hung adapter.

## Candidate closure and evidence

The destination must be new. Output filenames are positional
`edition-000.wav`, `edition-001.wav`, and so on; authored IDs never become output
paths. Each file is staged and published without overwrite. Cancellation is
checked again after staging, before visibility. `receipt.json` is published last
as the candidate's commit marker. A directory without that receipt is an
incomplete workspace, not a valid candidate package. M04 must still independently
check a complete package; receipt presence alone is not proof.

On failure, only operation-created outputs are retired and an empty owned
directory is removed. Unrelated files are preserved. Concurrent mutation of an
active candidate directory is unsupported; do not share its destination with
another writer. No retained asset, cache or previous candidate is deleted.

The **local private** receipt binds source references, container/decoded selected
and processed PCM hashes, compiled and execution recipe hashes, processing and
actual executable identity. Treat it like the private compiler plan: it can
contain project IDs, paths and private source hashes. It is not a shareable
receipt. The CLI prints only counts and fixed technical/authority status fields.
Normal diagnostics omit caller paths, text and private evidence values.

Producer structural success sets source-byte verification true and independent
checking, native admission and authority verification false. It grants no
listening pass, selection, upload, release or project adoption. M04 owns the
independent waveform/text check; M05 owns admitted retained publication.

## M03 test contract

`tests/spoken_programme_media.rs` generates only test-owned controlled signals.
The four full-stream goldens use the fixture's hand-calculated placements,
independently build expected interleaved samples, and decode output without
calling the production decoder. Neutral output must match exactly, including
all silence and unchanged regions. A separate overlap/gain/fade case compares
every sample with independent integer arithmetic.

Rejection coverage includes altered source bytes, a rehashed unsupported source
format, changed plan placements, protected-region fades, the peak ceiling plus
one, aggregate input limits, narrowed deadlines, malformed processing,
interruption, foreign-file preservation and existing-output refusal.
CLI smoke coverage uses one actual executable to compile and produce, verifies
the safe summary and canary-free normal diagnostics, and confirms a second invocation preserves every original
output byte. No source voice, provider or real speech is used.

```sh
cargo test --locked --test spoken_programme_media
cargo test --locked --lib browser_composition::pcm_window
cargo test --locked --lib song_edition::wave
cargo test --locked --test spoken_programme --test cue_relative --test audio_event_clocks
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

These are required invocations, not a claim they have passed. Record actual
results and the final merged parent in M03's role review before merging. Windows
and Linux hosted qualification is required. Synthetic PCM proves neither exact
spoken wording nor natural multi-speaker integration.
