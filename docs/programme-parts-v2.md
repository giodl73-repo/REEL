# Bounded native programme parts

`reel-episode-conform produce-parts` and `finalize-parts` add a portable native
path for programmes whose complete lossless representation cannot be assembled
within one admitted job. Existing conform manifests, ordered episode templates,
scene grammar, v1 checkpoints and monolithic/AAC delivery remain unchanged.

The new profile is `h264-independent-crf-yuv420p-bf0-mp3-continuous.v1`.
It retains complete lossless parts and encodes each picture once at the manifest's
selected CRF, with independent GOP/rate-control state. This is profile parity,
not decoded-pixel or packet equivalence to a hypothetical continuous encoder.
The finalizer copies video packets and performs one continuous stereo MP3 encode.
Select an MP3-compatible bitrate and 44100 or 48000 Hz explicitly through the
existing compact delivery controls; `retain_lossless_master` must be true.

Both commands run inside an explicitly staged asset job with `job.json`. Ordinary
shared Rust asset-job admission still accounts for all inputs, copied tools,
inline files, scratch and twice the retained output maxima under the existing
8-GiB bound. These commands do not alter admission, register objects, hydrate
sources, look up caches, or discover undeclared media. Account for the complete
continuous raw PCM scratch file and MP4 faststart overhead in the finalizer job.
No full-programme size claim follows from a successful small fixture.

```
reel-episode-conform produce-parts manifest.json --asset-root WORKSPACE --range range.json --output-dir NEW-DIR
reel-episode-conform finalize-parts set.json --asset-root WORKSPACE --output-dir NEW-DIR
```

`range.json` is a half-open contiguous global segment range (at most 21, keeping three outputs per segment plus one proof within the existing 64-output job limit), with
explicit global clocks, for example:

```json
{"start":1,"end":2,"start_frame":90,"start_sample":144000}
```

The manifest is the frozen **full** conform manifest, declared in each job.
Stage its selected authoring controls and only the media/source closure required
by that range. The original source verifier, geometry declaration, repeat-only
normalization, sample-clock padding and silent tail-hold implementation are
reused. The programme must select repeat-only output FPS and
`pad-silence-to-picture-boundaries`. No source is trimmed. Offsets are not accepted
on trust at delivery: the finalizer requires complete coverage from zero and
recomputes every global frame/sample boundary. Build ranges in order when the
preceding counts are not already known from native evidence.

For a subsequent range, stage the frozen manifest, the exact part proofs from
zero through the completed prefix, and their compact video and PCM. Declare
the prefix set in the staged job. Choose the next exclusive segment index,
then write the resulting range JSON into the next job's declared controls.

```
reel-episode-conform next-part-range PREFIX-SET.json --asset-root PREFIX-WORKSPACE --end NEXT-EXCLUSIVE-INDEX
```

The command verifies the prefix against the current producer and media tools,
checks its frame and sample clock, and prints the next range to standard
output. A gap, a changed proof or part, or an end outside the existing range
limits is refused. This command plans a range without rendering or changing
assets. Finalization still requires complete coverage.

On Windows, a `tig-publication` build can export a bounded producer recipe
from the prepared range workspace. Declare the local media runtime binding
alongside the manifest and range. The exporter verifies the declared workspace
and pins the worker, FFmpeg, FFprobe and configured runtime dependencies.

```
reel-episode-conform describe-part-job REQUEST.json --asset-root RANGE-WORKSPACE
```

The request stays outside the workspace. It names three workspace-relative
controls and sets the runner's byte and time limits explicitly.

```json
{"schema":"reel.programme-part-job-request.v1","manifest":"manifest.json","range":"range.json","runtime":"runtime.json","budget_bytes":67108864,"scratch_bytes":16777216,"timeout_seconds":180,"lossless_max_bytes":8388608,"video_max_bytes":8388608,"pcm_max_bytes":2097152}
```

These example budgets are for small fixtures. The output is a local job
recipe on stdout. Supply its exact inputs and installed tools through the
runner's resource authority, then admit it using the project's existing
runner and retention policy. A successful runner receipt provides the retained
pins for the next range or finalization. This exporter covers part production.

Each producer emits `lossless-NNNNN.mkv`, video-only `video-NNNNN.mp4`, raw stereo
PCM24 `audio-NNNNN.pcm`, and `part-proof.json`. The proof is distinct from a
same-workspace preparation checkpoint. It binds the exact parent manifest,
language, actual producer job/source scope, native validator and FFmpeg/ffprobe
hashes, profile, global indices/clocks, original selections, normalization/hold
counts, lossless YUV444/PCM identity and complete compact YUV420 identity. Local
lossless timestamps and compact packet PTS/DTS are checked. Each compact part
starts at random access, uses BF=0, and must have compatible SPS/PPS, color,
geometry, SAR, frame rate and timebase with every other part.

Publish **every** lossless, compact, PCM and proof output through the existing
immutable asset lifecycle. A retained part records its actual bytes/hash and
`cache://sha256/HASH`; protect all those dependencies/pins. Advancing a current
selection never removes an old part. The native producer does not itself claim
CAS publication occurred. The job runner performs normal registration/publication
only after output bounds pass.

Stage only compact videos, PCM, proofs and controls for finalization. Proofs may
be renamed in that job; compact/PCM paths in their immutable contents are relative
to the final input root. A set control contains ordinary hash-bound FileRefs:

```json
{"schema":"reel.programme-part-set.v2","manifest":{"path":"manifest.json","sha256":"...","bytes":123},"proofs":[{"path":"proof-00000.json","sha256":"...","bytes":456}]}
```

The ordered proof FileRefs are trust roots selected by the producing repository,
as with native scene proofs; this is not a signature or human approval system.
The finalizer freshly rehashes all declared compact/PCM/proof/control inputs and
checks profile/tool identity, contiguous full coverage, source selections and
hold/global-clock arithmetic. It redecodes compact parts, compares every decoded
frame of final MP4 against the complete ordered YUV420 part hashes/counts, checks
global PTS/DTS, and verifies exact PCM identity immediately before its one MP3
encode. All declared hold audio must still be zero. Adjacent cut audio steps and
black-picture findings remain producer-applied technical observations requiring
creative review. MP3 decoded clocks/counts are checked with a codec tail bound of
two 1152-sample frames; decoded MP3 is never claimed equal to original PCM.

The typed `reel.episode-conform-receipt.v2` uses
`lossless_master.representation = ordered-immutable-parts`. It records all actual
lossless cache references and consumed proof FileRefs, explicitly stating that
lossless custody is based on prior bounded producer verification. The finalizer
does **not** open or freshly rehash unseen lossless media. No manifest digest is
placed in `master_sha256`, and no nonexistent `master.mkv` is claimed. Native
episode captions accept this typed v2 timing evidence directly for SRT/VTT;
unmodified v1 consumers must continue rejecting unsupported representations.

Outputs are published only into a new directory after verification succeeds.
Failed/interrupted work has no acceptance receipt; original sources/parts remain
unchanged. Retry into a new output directory through a newly admitted job.
No creative, contextual listening, caption, rights, likeness/voice, principal or
release approval is inferred.

Complete immutable prefix editions use the separate
[prefix composition and browser master contract](programme-prefix-compositions.md).
That opt-in command emits an explicitly typed v3 receipt; ordinary v2 source
identity and current-executable validation remain strict.
