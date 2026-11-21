# Spoken-edition fixture and core test specification

**Status:** M00 specification, not an audio recording or implemented test suite.
Read with the [reuse map](media-expansion-m00.md) and
[test/evidence matrix](media-expansion-test-matrix.md). Original illustrative prose
below is public fixture text, not project canon or approved production speech.

## Six scripted performances, four editions

Module IDs stay stable across languages. Each row selects a distinct language
script/take; the same opening/closing take may serve both audience editions in
that language. `host` and `expert` are roles, not chosen voices or real identities.
No provider, voice, model or consent is selected by this specification.

| Module / speaker | English script | Spanish script |
| --- | --- | --- |
| `opening` / `host` | We can reuse reviewed speech in different editions. | Podemos reutilizar voz revisada en distintas ediciones. |
| `detail` / `expert` | Each edition selects exact takes and declares its speaker handoffs. | Cada edición selecciona tomas exactas y declara los cambios de hablante. |
| `closing` / `host` | The result keeps its own timing and review evidence. | El resultado conserva sus propios tiempos y la evidencia de revisión. |

All editions communicate a production capability under investigation; they are
not claims that an implementation is qualified today. Technical editions include
the detail; executive editions omit it. `closing` requires `opening`, not
`detail`; the technical brief additionally requires `detail`. Negative fixtures
introduce a closing with an unresolved `detail` dependency to test rejection.
Human source review still checks whether these declarations preserve meaning.

### Delivery briefs for a later spoken demonstration

| Edition | Modules | Desired spoken duration, including joins | Listening/access context |
| --- | --- | --- | --- |
| Executive EN | opening, closing | 6–15 seconds | Private desktop briefing, headphones and small mono speaker; speaker-role transcript and module chapters |
| Technical EN | opening, detail, closing | 10–25 seconds | Same devices; complete explanation and declared host/expert handoffs |
| Executive ES | opening, closing | 7–18 seconds | Same devices; independently reviewed Spanish wording/joins/text access |
| Technical ES | opening, detail, closing | 12–30 seconds | Same devices; Spanish detail with its own measured clock |

Reference delivery is a local stereo PCM24 WAV at 48,000 Hz, accompanied by UTF-8
speaker-role transcript JSON and module-chapter JSON. No picture or network
service is required. The transcript preserves exact observed words and relevant
non-speech information, distinguishes uncertainty, and uses edition-local sample
intervals. Adapted scripts, observed performance and recognition remain separate.
PCM bytes and metadata need their own exact input/output identities.

Spoken review targets: integrated loudness −18 LUFS ±1 LU for the finished spoken
programme and sample peak at most −1 dBFS, using a pinned measurement runtime.
Loudness processing, if needed, creates an explicit derivative/envelope with its
own evidence; it must not mutate a take or change word timing. Review masking,
word fidelity and handoffs by listening; a metric does not establish them.
M03's neutral engineering fixture below has no loudness normalization or claim
to meet the spoken loudness target. No hidden time stretch to meet duration ranges.

## Exact engineering clock and signal oracle

These are **controlled signal durations**, not measured or predicted speech.
At 48,000 Hz, stereo, signed 24-bit interleaved little-endian PCM, full takes
have the following lengths. Two channels carry distinct patterns to expose
swaps or accidental downmix. For zero-based sample `i` and language/module code
`k` (EN opening/detail/closing = 1/2/3; ES = 4/5/6):

`left(i) = 1000*k + (i mod 97) - 48`; `right(i) = -left(i)`.

The first 2,400 and last 4,800 samples of each take are recorded silence (zeros),
overriding that pattern. Retain all samples. A fixture-only protected word region
is `[2400, duration-4800)`; protected breath/tail is `[duration-4800, duration)`.
These labels test boundary protection and are not evidence of audible words or
breaths. The initial silence and trailing silence must not be trimmed implicitly.

| Language | opening | detail | closing | Added host→expert / expert→host pause | Executive host→host pause |
| --- | --- | --- | --- | --- | --- |
| EN | 144,000 (3 s) | 192,000 (4 s) | 144,000 (3 s) | 14,400 / 14,400 | 12,000 |
| ES | 168,000 (3.5 s) | 216,000 (4.5 s) | 168,000 (3.5 s) | 19,200 / 19,200 | 16,800 |

All intervals are half-open. Selected takes use full source intervals. The next
start is the previous placed end plus the explicitly added gap. Recorded silence
is already inside the source duration; do not add it again. For this fixture,
no gain, fades, resampling, channel mapping or overlap is applied. The full output
PCM equals the ordered source PCM with zero-filled declared gaps, byte for byte.

| Edition | opening span | detail span | closing span | Exact output samples / seconds |
| --- | --- | --- | --- | --- |
| Executive EN | [0,144000) | omitted | [156000,300000) | 300,000 / 6.25 |
| Technical EN | [0,144000) | [158400,350400) | [364800,508800) | 508,800 / 10.6 |
| Executive ES | [0,168000) | omitted | [184800,352800) | 352,800 / 7.35 |
| Technical ES | [0,168000) | [187200,403200) | [422400,590400) | 590,400 / 12.3 |

Also test no-added-gap concatenation and an **explicit** overlap of 2,400 samples
between fixture-only unprotected intervals. Use a separate signal variant with
protected regions outside the overlap; sum channels with checked integer
arithmetic, reject 24-bit overflow instead of clipping. The unauthorized overlap
of protected word/tail regions is a rejection case. M06's multi-cue variant splits
`detail` into two explicitly speaker-tagged cues whose total source-plus-gap
duration is declared separately; it cannot silently reuse the baseline goldens.

Compile-time arithmetic uses checked u64 samples/u128 products, never floats for
identity. Progress anchors use declared nearest-half-up rounding; interval frame
starts use floor and ends ceiling at 25/1 fps when a visual target is tested in
M10. Sample intervals remain authoritative; frame rounding does not retime audio.

### Bounded engineering execution profile

- A declaration is at most 256 KiB; at most 16 modules, 64 total cues, 8 language
  variants and 16 editions. Fixture-specific caps: six baseline takes, four
  editions, at most 15 seconds/720,000 samples per output, 16 MiB total input PCM,
  64 MiB final packages and 128 MiB peak scratch. Budget checks precede execution.
- Cooperative timeout is 30 seconds per fixture producer/checker operation,
  with a measured 60-second outer fixture budget checked when the test completes.
  Trusted synchronous test I/O must return; this is not a process sandbox for a
  hung operating-system call. Repeat with an intentionally tiny cap to prove refusal.
  No overwrite; new destination only. Failure cannot leave a usable final package.
- The baseline output is lossless WAV PCM24; decoded PCM sample tolerance is
  exactly zero. Container headers may differ; verify container and decoded
  identities separately. Lossy delivery is excluded until a separately reviewed
  codec/delay/tolerance profile is added; no placeholder lossy tolerance passes.
- For a separate gain/fade fixture, use rational gains and signed integer division
  toward zero. A linear entrance of N samples uses `i/N`, and an exit uses
  `(N-1-i)/N`, for `i=0..N-1`. Protect declared intervals before applying an
  envelope; compare the complete decoded expected signal with zero tolerance.
- Engineering peak limit is absolute sample magnitude ≤4,194,303 (below −6 dBFS)
  after all declared transforms. Test the limit and one sample above it. This is a
  conservative fixture/profile constraint, not a universal production peak rule.
- Support core validation/compilation on Windows and Linux. Qualify actual media
  execution per platform with the FFmpeg/FFprobe executable hashes and version
  recorded by the test runner. Current local tools are Rust/Cargo 1.98.0 and
  FFmpeg `N-126390-g9fc8c785e2-20260902`; CI runtime identity must be recorded there,
  not assumed equal. Missing runtime is not run, never a successful media skip.
- Native protected-byte/retained-store integration uses only the pinned SDK's
  supported platforms (currently Windows for this input path). Linux must prove
  explicit unsupported refusal there; pure edition/core PCM tests still run on
  both. Do not weaken protection to manufacture parity.

## Exact test inventory and invocation ownership

Targets and test names prefixed `spoken_programme` below are **planned**, not
existing commands claimed to pass. M01 creates its target/fixtures; later PRs add
only their owned cases. Use Rust fixtures and existing runtime ports, no Python
helpers or new network test service. Unit tests write tiny generated signals only
under test-owned temporary directories, never an asset cache.

| PR | Target / required test names | Oracle or failure expectation |
| --- | --- | --- |
| M01 | `spoken_programme`: `validates_four_editions_and_multicue_modules`, `rejects_unknown_duplicate_stale_and_unscoped_declarations` | Strict contract/ID/source/text/context closure; no process/network/storage calls |
| M02 | same target: `compiles_four_exact_sample_plans`, `rejects_context_marker_trim_overlap_and_clock_errors` | Golden table above; source evidence identity and protected regions; checked overflow and no implicit retiming |
| M03 | `spoken_programme_media`: `neutral_pcm_matches_independent_oracle`, `declared_gap_overlap_and_envelope_match_pcm`, `failure_never_publishes_candidate` | Rust-generated signal/independent expected PCM, boundary and full-stream comparison; mutation/format/cap/interruption/no-clobber failures |
| M04 | same media target: `checker_reopens_media_and_rejects_rehashed_tamper`, `four_transcripts_and_chapters_follow_edition_clock`, `portable_output_redacts_canaries` | Recompute superficial report hashes after bad source/placement/media/text edits; checker still refuses; role and interval matching; no canary disclosure |
| M05 | `spoken_programme_history` with `tig-history`: `spanish_detail_change_preserves_other_consumed_recipes`, `retained_v1_reopens_and_stale_reviews_refuse`, `unsupported_protected_input_refuses` | Compare all four recipe identities independently of aggregate index; original pins reopen or explicit missing/unsupported diagnostic, never restoration |
| M06 | `spoken_programme_media`: `qualification::module_with_two_speakers_matches_sample_and_access_goldens`, `qualification::multicue_adjacency_scope_and_protected_regions_refuse_before_output`, `qualification::reordered_modules_need_fresh_controls_and_checked_adjacency`; separate governed listening pack | [Concrete multi-cue clocks and pack](spoken-edition-qualification-v1.md); the media target extends M01's declaration-only multi-cue proof with actual PCM and access checks. Technical result remains distinct from human decisions |

Exact commands after the owning PR creates each target:

```sh
cargo test --locked --test spoken_programme
cargo test --locked --test spoken_programme_media
cargo test --locked --features tig-history --test spoken_programme_history
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
git diff --check
```

M01/M02 core tests cannot invoke media or providers. M03/M04 explicitly invoke
only pinned FFmpeg/FFprobe as necessary. Side-effect tests use injected failing
ports/counters and fixture canaries. Exact CLI smoke commands use the reserved
names in the reuse map; CLI validation failure is nonzero with a static diagnostic
class and safe relationship identifier, without private input text/path values.
Do not claim CLI coverage merely from library tests.

Useful **existing** baseline regression commands, selected only where affected:

```sh
cargo test --locked --test cue_relative --test audio_event_clocks --test episode_captions
cargo test --locked --test scene_delivery
cargo test --locked --features tig-history --test tig_publication --test tig_episode
```

Cargo metadata confirms these six existing target names. M00 inspected their
source and did not run their Rust/media suites. M03–M05 must record actual test
results, versions/platforms and relevant regression selections; a source read is
not a passing execution. Hosted required checks remain mandatory for every PR.

## Human review remains unperformed

Before real speech is produced or reused, the producing project records exact
language/script, selected voice/performer, permitted audience/reuse operation,
source/consent and provider/runtime scope. This public specification supplies none
of those authorities. Spanish fixture prose needs actual language approval for a
production use; an assistant-written translation is not that approval.

Review full EN/ES editions and all joins under the matrix's human rubric.
Record measured words/timing and duration, pronunciation, meaning, speaker
handoffs, breaths/tails, intelligibility, transcript access and the exact reviewed
mix. No synthetic signal, green CI or numerical loudness value closes that gate.
