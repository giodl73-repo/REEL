# Recording clocks over existing Tig inputs

Entry REEL `dab326645040513fa2fd984508def614aa163377`. This integration
fixture connects the explicit audio-event contract to the existing Tig
input provider; no new cache, media grammar, renderer or producer API is added.
The bounded synthetic source contains two pictures and one nonzero PCM recording.
The music event supplies the clock without narration cues or speech alignment.

The regular Windows case changes one picture on main, refuses an old job against
that new source before payload reads, then admits its explicitly updated pin.
The original metadata archive still resolves the old picture after Git, records
and definitions become unavailable. Exact input guards survive dropping the
source and refuse a write while planning the original recording-clock job.

The explicit native FFmpeg case renders from the archived primary metadata with
Git unavailable, runs the existing independent delivery checker, then compares
every decoded music PCM byte and picture pixel against synthetic source values.
Neutral black/white pictures avoid assuming saturated RGB values survive the
declared RGB/YUV conversion unchanged. Initial saturated-color equality refused;
the fixture corrected that expectation without changing renderer semantics.
CI invokes this case explicitly on Windows, where required mutation protection
is implemented. Other platforms do not gain a path-protection guarantee.

Sound/editor/rights producer review: no fabricated speech, time stretching,
private media, publication or creative approval. Local job/contract/production
controls remain explicit files, hash-bound by the existing planner; this fixture
does not archive those controls or establish complete managed-job enrollment.
Current source semantics, source-owned VFX timing, complete music-video authoring,
resource admission, retained result claims and actual consumer adoption remain
separate. This check supports reuse of the existing path, not full CAMINOS cutover.

Local result: one regular integration test and one explicit native FFmpeg test
pass. The former's first run also caught absent synthetic selection states;
those were explicitly declared rather than weakening resolver validation.
Hosted qualification and runtime adoption remain separate.
