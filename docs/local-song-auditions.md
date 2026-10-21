# Explicit local song auditions

An optional campaign-slot `max_duration_seconds` bounds adaptive retries. It must
be at least the initial `duration_seconds` and at most 480. Longer timing/audio
references fail rather than overriding this ceiling. For compact new recordings,
use one source pass, explicit singer/tempo/phrasing briefs and a fixed ceiling;
lyric qualification alone does not establish musical pacing or voice delivery.

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
