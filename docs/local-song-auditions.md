# Explicit local song auditions

An optional campaign-slot `max_duration_seconds` bounds adaptive retries. It must
be at least the initial `duration_seconds` and at most 480. Longer timing/audio
references fail rather than overriding this ceiling. For compact new recordings,
use one source pass, explicit singer/tempo/phrasing briefs and a fixed ceiling;
lyric qualification alone does not establish musical pacing or voice delivery.

Use repeated `--audio-guide-target-slot ID` options to restrict a generated guide
to named same-source candidate slots in a multi-poem campaign. Other slots stay
original text2music and carry no guide audio inputs/dependencies. Unknown targets
and mismatched source/version locks fail before any recording starts.

`--takes-per-slot N` runs a bounded serial round (1–10 completed takes for each
pending slot). A failed brief does not hold the rest of the campaign indefinitely.
Unqualified slots remain pending; `round-complete` is distinct from campaign
completion. Resume another explicit round to continue; qualified takes stay fixed.

Generated-guide covers expose two separate installed ACE controls.
`--audio-guide-strength` blends cover conditioning; it does not preserve waveform
initialization by itself. `--audio-guide-noise-strength` explicitly sets source
initialization (0 starts from noise; 1 stays closest to the source). Omission
preserves the historical zero setting. Both are recorded with the same-source
proof; judge actual lyric coverage and musical variation, not the knob values.

`--brief-asset` adopts a pinned `reel.song-musical-briefs.v1` payload whose `slots`
map binds each replacement `caption` to the exact original `source` and
`source_lock`. Its descriptor includes an immutable `version_lock`. Adopt after
draining a complete take, never by editing the frozen campaign or old requests.
New brief revisions cannot target already-qualified slots; resuming the same
revision preserves candidates qualified under it. Future take packages retain
the brief payload/dependency as well as the exact submitted caption.
Entries may also scope the existing `repeat_section_lines` (0 or 2–8) and
`phrase_max_words` (4–16) options. These preserve canonical words and cannot alter
the slot's hard duration ceiling. This allows a close take to keep one pass while
other pending voices try denser supplied repetitions at their original lengths.

`tools/local_song_edit.py` produces explicit short editions inside a native Rust
job from one hash-bound generated source, ordered nonoverlapping cuts and bounded
instrumental fades. It verifies every evidence-bearing decoded sample is exactly
unchanged, then remaps the original independent word evidence to the edit clock.
Changed fade edges and incomplete word spans supply no evidence. This is a new
edition with source-derived analysis, not a new performance or transcription.
Retain the original request, recording, generation/analysis, source version, edit
recipe and equality receipt. The review page labels the edit and original prompt.

`tools/local_song_campaign.py` coordinates explicit project campaigns through
`reel-assets`: ingestion, registration, pinned immutable versions and `run-job`.
`tools/local_song_backend.py` is the trusted offline ACE/Whisper/Demucs adapter
executed by that Rust job contract. Private poems, briefs, source locks, requests,
recordings and diagnostics belong in the producing project's cache/LFS.

This first adapter supports Spanish original `text2music`, direct generation
without an optional semantic planner, and the already installed ACE turbo
float32/eager/w8a8_dynamic runtime. It starts no service and downloads no model.
The installed Python environment must already contain ACE, Torch, Demucs,
Transformers, NumPy and SoundFile. Installed model and source files are explicitly
hash checked; the runtime manifest retains package versions and model custody.
Environment flags enforce the libraries' offline lookup mode, not an OS network
sandbox. No voice reference, cover input, upload or release path is implemented.

The generic Rust job contract now accepts optional `installed_tools`, mapping
tool names to absolute `path`, `sha256` and `bytes`. These must be registered and
are freshly checked before admission and immediately before execution. The EXE
stays with its installation/DLLs; the job runner still owns staging, reservations,
process-tree timeout and result publication. Installation dependencies are part
of the caller's frozen runtime identity. This is trusted execution, not isolation
against malicious manifests or concurrent modification of a software installation.

## Run

Build `cargo build --release --locked -p reel-assets --bins` using the existing
external Cargo target. Pass a supplied private project root with its configured
existing caches, and a private working directory inside that project:

```text
python tools/local_song_campaign.py --root PROJECT --assets REEL_ASSETS_EXE --work PRIVATE_WORK snapshot --ace-root ACE_INSTALL --whisper-root WHISPER_SNAPSHOT --demucs DEMUCS_CHECKPOINT --python ACE_PYTHON --ffmpeg FFMPEG_EXE
python tools/local_song_campaign.py --root PROJECT --assets REEL_ASSETS_EXE --work PRIVATE_WORK run --campaign CAMPAIGN_JSON --runtime-asset PRIVATE_WORK/runtime-asset.json
python tools/local_song_campaign.py --root PROJECT --assets REEL_ASSETS_EXE --work PRIVATE_WORK restore --state PRIVATE_WORK/state.json --lfs-executable REEL_LFS_EXE
```

Snapshot is explicit asset admission: it freezes installed inputs in the supplied
cache and registers/pins an immutable runtime. It is not an orientation command.
Never run setup or snapshot implicitly when opening a project.
An interrupted snapshot checkpoints completed admissions. `--metadata-retries N`
explicitly permits at most N retries of a busy metadata lock; it never retries a
corrupt object, changed source, failed job or unsupported input.

The campaign uses `reel.song-audition-campaign.v1` with explicit `slots`. Each
slot supplies a unique `id`, `title`, `label`, `style`, `caption`, `source`
SHA-256/bytes/cache URI, exact `source_lock`, `duration_seconds` and `seed_base`.
Canonical lyrics are read from that freshly verified source. Engine stanza tags
remain separate. The source bodies must already include any title that is a verse.

Each slot records and analyzes one take at a time. A retry changes the seed;
after three failures duration increases by 15 seconds, capped at 480. Every
actual request is retained before execution. No retry rewrites lyrics or silently
changes the brief. An infrastructure error stops the run for diagnosis.

Analysis fully decodes the recording, checks duration and finite signal, estimates
vocals with the explicit Demucs checkpoint, and runs three unprompted Whisper
passes: mix at 30/5-second chunk/stride, mix at 20/3, and cropped estimated vocals
at 20/3. Reports preserve raw word timestamps and exact source lines. Chunks are
batched four at a time on the GPU; reports retain that setting alongside the
chunk/stride conditions and unprompted generation parameters.

The current diagnostic policy permits performed repetitions and at most one
unrecognized word in each source line's best complete recognition pass. Every
line must have at least one recognized word, including one-word lines. Match
near-contiguous phrases, with at most two intervening recognized words and a
bounded time span, rather than collecting words across the song. Observed tokens
are not reused for two source occurrences. Continuous source-order alignment is
retained as a separate diagnostic, since it can discard a phrase that appears
in another repeat. The listening page shows both results. This remains
recognition evidence; it can mishear, and does not grant musical selection,
pronunciation approval or release. The producing project must authorize its gate.

Every completed take has a pinned generic artifact package containing the raw
recording, preview, generation receipt, both logs, vocal estimate, line analysis,
raw transcripts, exact request and job/runtime identities. Failed lyric screens
are retained. `state.json` resumes completed slots only with the same frozen
campaign/backend/runtime. `--adopt-backend-revision "reason"` explicitly records
an adapter upgrade and both hashes while preserving old requests, evidence and
qualified slots. A changed campaign or runtime still requires a new campaign.
`request-stop` asks the coordinator to stop after a completed take and analysis;
`request-stop --clear` clears that request before resuming. A static private
`review.html` is refreshed as slots
qualify; it starts no server. Current production selections are untouched.
An interruption after version registration can resume the retained take through
explicit-version verification without regenerating the recording. Unselected
auditions never require promoting a Golden just to satisfy a current lookup.

`restore` exercises the actual local Git LFS path for raw recordings, previews
and line reports, using invocation-only transfer overrides for the supplied
project root. It changes no shared Git configuration and never probes an archive.
Use `--all-attempts` to include rejected takes in a retention rehearsal.

The explicit `--repeat-after-failures N` option supplies two unchanged full-poem
passes after N failed takes and increases the requested duration. The policy
revision and render-pass count are retained. Canonical source text and unique
source-line count never change. Adapter validation rejects arbitrary substituted
words or partial lyric rewrites.

An explicitly supplied `--planner-asset` adopts a separately frozen installed
ACE 5Hz planner component for future takes. Its manifest, installed file hashes,
cache/LFS custody and version lock are required. A separate offline planning
process produces duration-checked semantic codes, then releases GPU memory
before audio generation. The plan and all three step logs are retained. This
policy supplies one unchanged full-poem pass, preserves qualified candidates,
and keeps the same unprompted line gate. It does not download models or rewrite
lyrics. Planning must be evaluated on actual recording evidence.

`--generation-route direct|planned` explicitly records a route change for future
takes, retaining the old component and results. `--repeat-section-lines N`
supplies each consecutive source section twice, in order, with no replacement
words. Original stanza boundaries remain separate. Render validation checks the
exact expected word sequence; canonical source lines and acceptance do not
change. This uses the same owner permission for performed repetitions.

`--phrase-max-words N` joins consecutive short source fragments into fuller
sung phrases, respecting original stanza and sentence boundaries. It changes
rendered wrapping only. Exact-word validation, canonical line accounting and
retained source bytes remain unchanged. The accompanying shorter diction
caption requests syllabic phrasing and minimal melisma; its musical outcome
still needs the same full recording analysis and owner listening.

`--asr-dtype float16` explicitly selects GPU recognition precision for future
requests; the default remains float32. The request, raw passes and report record
the actual precision. Model weights, three unprompted conditions, beam count,
canonical source and acceptance remain fixed. Verify an actual retained private
fixture before adopting a precision change; preserve its old report alongside
the new analysis. Generation precision is a separate setting.

`--timing-reference-slot ID` explicitly reuses a qualified same-source request's
duration and starting seed for future distinct musical briefs. Subsequent retries
advance the seed, so a failed take is never regenerated identically. The frozen
reference request/version and per-slot adoption attempt are retained and checked
on resume. Packages lock the reference dependency. This supplies no reference
audio, changes no source words, and makes no quality guarantee for another brief.

`--focus-missing-lines` adds a bounded exact-source phrase reminder to generation
from the best prior take's missing lines. The report must match every canonical
source line. The request/package retain the prior report and version dependency;
the reminder cannot rewrite source or rendered words. Recognition receives no
lyric cue or canonical prompt, and the same line criterion still decides whether
the new recording qualifies. `--no-focus-missing-lines` disables this policy.

The optional `--ctc-runtime-asset` adds a separately frozen, already installed
Spanish acoustic recognizer for near-pass uncertainty. After the primary three
passes, up to three failed lines may receive bounded CPU checks around neighboring
recognized spans. Each window is a separate pass: words are never joined across
windows to fill a line. The decoder has no language model, source prompt or
canonical lexicon. Retain the original primary report, new acoustic transcript,
exact windows/model hashes and merged analysis. Qualifying or distant-from-pass
takes skip loading the extra model. No model is downloaded. A private analysis
revision recovered two lines on an unchanged 35/37 take, yielding 37/37; its
original report remains immutable. This is machine evidence, not human approval.

`--audio-guide-slot ID` explicitly uses an already qualified, generated
same-poem text2music recording as a cover guide for a distinct musical brief.
The adapter verifies source/version, private permissions, original request and
generation-receipt/audio hashes before loading the engine. It retains the exact
guide render and source duration. `--audio-guide-strength` declares influence
(0.2–1.0; default 0.75). No living-person source or named voice is inferred.
The new output is labelled an audio-guided variation, with guide assets and
dependency locks; it still requires fresh unprompted coverage and human musical
review. Fixed candidates and original guide bytes remain unchanged.

A qualified generated cover may itself guide another brief. Its request must
retain the identical poem/version and private scope, and its original generation
receipt must bind the waveform and record cover conditioning without external
upload. The new package locks that donor version and its existing native
dependency closure; this does not admit an external or identifiable voice source.

A qualified sample-preserving short edition may also supply that guide. Retain
its explicit recipe and native edit receipt in addition to the original
text2music request/generation receipt. The adapter verifies every hash binding,
identical source/private scope, unchanged-sample evidence, unchanged tempo/pitch
and the exact edited duration before generation. The cover receives one unchanged
poem pass and the edited duration; it is a new guided performance and needs fresh
recognition. The edition remains labelled as an edit, with no new recognition or
human approval claimed for its source-derived evidence.

The backend's `recount` operation takes a hash-bound prior report, recording,
request and source through a Rust job. It revises only line accounting from the
retained unprompted token/timestamp passes, retaining the prior report and a
recount receipt. No new audio or recognition is created. Publish a new immutable
analysis version; never rewrite a prior diagnostic in place.

## Verification

Run `cargo test --locked -p reel-assets`, clippy with `--all-targets -- -D warnings`,
and `python -m unittest discover -s tools -p test_local_song_backend.py`.
Synthetic cases cover installed runtime execution and output retention, absent
or unregistered tools, child failure/timeout, repetitions, missing lines and
single-word fragments. Private recording, actual local LFS restoration and
historical lookup must be verified by the producing project before adoption.

Review uses producer-applied lyric/vocal, music reconstruction and
rights/provenance checklists. Machine evidence and human listening stay separate.

## Full-recording vocal estimates

`local_song_vocal_estimate.py` accepts an explicitly pinned original recording,
local Demucs checkpoint, runtime files/packages and complete duration. It runs
offline as a declared-output job, checks original bytes before and after, and
retains the full stereo source clock. No setup or model download is implicit.
The result is a derivative estimate with possible accompaniment bleed, not an
original isolated stem or a generated/re-pitched/re-timed performance.

Normalization distributes the source mean across the model's source count.
Unit checks cover that restoration and reject invalid duration/scope/locks.
Producing projects retain their actual complete-recording qualification receipts;
technical qualification does not approve the estimated vocal or a new arrangement.


## Recording-timed bilingual album cards

Do not divide a song duration by text length to produce lyric-card timing.
Instrumental introductions, sung pauses and repeated sections need their actual
recording clocks. The album exporter accepts `performed-phrase-pages` alongside
the historical editorial-reading mode. Each displayed pair requires retained
phrase start/end evidence; the exporter rejects a card entering after its phrase
or cutting more than 200 ms from its tail. Frame conversion uses the album's declared sample rate.

`local_song_timing_conform.py` compares three non-silent windows before a retained
timeline crosses encodings. All windows require correlation at least 0.95,
consistent offsets within 40 ms, and complete duration agreement within 100 ms.
Apply the measured offset explicitly. Different takes and short/full editions
require their own evidence, even when their canonical poem is identical.

`local_song_lyric_transcript.py` runs two unprompted word-clock passes through an
explicitly frozen local recognizer. `local_song_lyric_pages.py` proposes cards
from retained performance events or phrase candidates, preserving repeats and
clearing longer gaps. It leads a card by up to 400 ms, keeps paired languages on
one clock, and records literal evidence for partial/altered phrases. Long held
syllables are permitted only when explicitly requested for retained timelines.
When canonical-context preservation is explicitly requested, unmatched lines stay beside a neighboring measured phrase, labelled as context without their own clock. Earlier context may receive a four-second reading lead. Canonical line IDs and text remain unchanged. Unmatched canonical lines receive no invented clock. These are phrase cards,
not word highlighting or new proof that every canonical word was sung.

Run `test_local_song_lyric_pages.py` and `test_local_song_album_mp4.py`. Tests
exercise wrong-performance and short/full rejection, repeats, instrumental gaps,
non-48-kHz clocks, late-card rejection, and actual rendered-card appearance in
both columns. The producing project must still bind its exact selected recordings,
retain actual job/codec evidence and review uncertainty; no human listening or
new lyrical approval follows from these checks.


## Compact chaptered collection copies

`local_song_compact_collection.py` joins explicitly locked album videos into one
size-bounded listening copy. Source audio is verified against each master PCM
hash/frame count, joined at exact 48-kHz sample boundaries and encoded once as
stereo AAC. A pre-encode PCM hash proves that the join did not trim, normalize,
resample or retime the selected audio. This is a lossy listening derivative,
not a replacement source or a new musical Golden.

Matching H.264 video streams are copied without encoding. Video-only remuxes
isolate MP3 priming metadata before concatenation, and declared audio sample
counts override rounded picture-container durations. Every output video packet
payload and its shifted presentation clock must match the sources. Chapter
entrances follow those same sample counts. A complete final decode and strict
decimal-byte ceiling precede publication; nonmatching streams or an insufficient
budget fail. Retain source/output packet probes and codec evidence in cache/LFS.

Run `test_local_song_compact_collection.py`; its fractional-duration chapters
exercise MP3 priming, exact picture payloads/clocks, PCM joins, AAC padding,
chapter labels and a rejected bitrate budget. Recheck the shared album-master
fixtures when changing its optional streaming hash accumulator.

## Scoped stanza roles and stricter line acceptance

A frozen musical brief may declare `stanza_roles`, one `Verse`, `Chorus` or
`Bridge` per original stanza. This changes engine tags only; canonical Spanish
bytes and word order remain unchanged and the backend validates them. Roles
require a single direct source pass, with no supplied repeated sections or guide.
The same explicit pending-slot revision may set `duration_seconds` and
`max_duration_seconds`; invalid bounds or a duration above its declared ceiling
fail before generation. The original campaign and completed takes stay immutable.

`run --max-one-word-gap-lines 3` retains the original complete-line gate and
additionally limits the number of lines with one unrecognized word. The cap is
recorded in state acceptance revisions, take summaries and package metadata;
original recognition reports stay intact. Tightening the cap rechecks previously
qualified slots. Human musical selection remains separate. Run
`test_local_song_acceptance.py` and the existing backend/campaign fixtures.

For a fresh take whose combined independent model closures exceed the native
single-call read cap, `--max-verification-bytes` explicitly opts into bounded
verification in parts. Native metadata checks and idempotent exact registration
identity validate the immutable input/output partition and dependencies. Each
dependency then passes normal native closure verification within its unchanged
cap; root primary/output objects pass native active-cache hashes. A separate
receipt records the aggregate byte budget and all checks. Storage and Git limits
are unchanged; corruption, mismatched identity and insufficient budget fail.
Historical references stay deferred. Run `test_local_song_verify_parts.py`.
