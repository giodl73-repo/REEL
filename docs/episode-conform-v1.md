# Data-driven episode conform

Long native jobs can use two bounded steps with the same checked source closure:

```
reel-episode-conform prepare conform.json --input-root . --asset-root . --output-dir prepared
reel-episode-conform finish prepared --input-root . --asset-root . --output-dir out
```

When normalization itself exceeds one bounded step, a closed native job can
declare multiple `prepare-batch` steps followed by assembly and finish:

```
reel-episode-conform prepare-batch conform.json --input-root . --asset-root . --output-dir prepared --max-segments 3
# Repeat the same command ceil(segment_count / 3) times in the frozen job.
reel-episode-conform assemble-preparation prepared --asset-root .
reel-episode-conform finish prepared --input-root . --asset-root . --output-dir out
```

Each batch performs the existing upstream checks and normalizes only the next
ordered group. It retains `normalization-progress.json` and verified generated
media inside the same job workspace. Subsequent batches rehash every original
input and retained intermediate and require the same manifest, closed job and
validator/FFmpeg/FFprobe bytes. Completed media is reused without re-encoding.
The progress record is explicitly incomplete: it has no assembled master,
`prepared.json`, final receipt or delivery. Assembly rejects incomplete coverage;
finish still performs all ordered decoded picture/PCM, timestamp, boundary and
encoded-delivery checks. Existing one-step preparation remains unchanged.

The steps keep the original per-step timeout and operation/storage limits.
These are checkpoints within one frozen job, not a cross-job resume API.
Default job retirement still applies on failure; preserving failed debugging
scratch requires an explicit bounded retention decision and later disposal.

Preparation requires a closed native job workspace (`job.json`). It verifies the
declared input/tool/inline-file hashes, rejects undeclared files and links, runs
the complete upstream/normalization checks, and retains its exact normalized
segments and lossless assembly. `prepared.json` explicitly records that the full
episode is still unverified. It is neither a final receipt nor creative approval.

Finish rehashes every original input and retained intermediate, requires the same
validator/FFmpeg/FFprobe bytes, and verifies the prepared order and clocks. It then
performs the same complete ordered picture/PCM comparisons, timestamp/boundary
checks, encoding and full encoded-delivery decode as ordinary `build`. The final
receipt binds `preparation_sha256`; the exact preparation record is copied into
the final output. Preparation sources remain unchanged. Both steps use the
existing job limits; this mode does not extend a timeout or omit a check.

An explicit `finish-consume-normalized` command uses the same arguments as
`finish`. It requires preparation directly inside the same closed job workspace.
After the complete ordered picture/PCM, timestamp and boundary checks succeed,
it rehashes and retires only the exact generated `normalized-NNN.mkv` and
`padded-NNN.mkv` files inside that preparation's temporary subdirectory, before
compact encoding. Original inputs and the assembled master remain unchanged;
ordinary `finish` continues to preserve all preparation sources. The final
receipt records retired file/byte counts. A consumed preparation is one-shot;
an encoding failure requires fresh preparation. This is temporary execution
storage, not cache-object deletion, tiering, selection or creative approval.

Preparation directories are owned job scratch. Reusing one after that scratch is
discarded requires explicit retention of its referenced intermediate bytes; the
final receipt alone does not claim that those temporary files remain available.

Decoded picture checks use FFmpeg's SHA-256 hash muxer over packed raw video,
with an independent raw-packet byte/frame counter. Ordered segment cuts count
decoded frames rather than seeking timestamps. The SHA-256 values are identical
to the historical raw-video pipe checks; short/extra frames remain errors.

An optional `preassembled_master` FileRef supplies a read-only assembled
candidate. REEL still validates every current source, compares every ordered
decoded picture/audio segment, checks geometry/rates/timestamps, and fully decodes
the final compact delivery. The supplied file is never renamed or deleted.
Retaining a lossless output makes a separate copy.

A scene segment with `master_proof` may explicitly pin its original checker with
`master_proof_validator`. This FileRef must match the actual checker bytes and
the proof's validator SHA-256. All current plan, media-tool, receipt and consumed
master checks remain mandatory. Omitting it retains the strict current-executable
check. Neither option grants human approval.

`reel-episode-conform` builds one language-local lossless FFV1/PCM24 master from
selected scene and presentation masters:

```text
reel-episode-conform build <manifest.json> --input-root <authoring-root> --asset-root <hydrated-media-root> --output-dir <new-dir>
```

`reel-scene-build execute-episode` can drive the same conform after its
changed-only scene run. Its episode-build manifest names a scene index and an
ordered conform template; scene segments name a `scene_node_id` and exact
delivery job, and the executor fills master and receipt hashes from the
verified final scene state. This supports both fresh scene builds and exact
prior-output reuse in one command. Selected presentation masters remain
explicitly hash-bound in the conform template.

The manifest schema is `reel.episode-conform.v1`. It names the episode and
language (`es` or `en`), output sample rate, selected catalog, exact generic
master-order definition, optional source master template, episode authoring,
season and episode binding files, and an ordered `segments` array. Every file
reference has relative `path`, `sha256`, and `bytes`. Scene segments name their
scene ID and may name the template role they perform (such as `opening-poem`).
Episode presentation segments name their role (such as `series-opening`,
`chapter-title`, or `end-credits`). Each segment binds a selected master and
its source receipt. Scene receipts must be `reel.scene-build-receipt.v1`;
presentation receipts must be `reel.presentation-master-receipt.v1` and agree
with the selected scoped master binding and template definition. The conform
requires their decoded frame/sample counts, verified timestamps, and a declared
technical validation route. `reel-presentation-adopt` supplies one route for a
selected existing master; newly rendered presentation can use another checked
route. A `decoded-source-equivalent` presentation segment must also bind its
exact `adoption_manifest`. Conform reruns that selected adoption in a temporary
directory, compares its receipt fields and fully decoded content with the
selected master, and records `upstream_presentation_verified`. This recheck is
deliberately expensive for long, high-resolution segments; a higher build graph
may reuse an exact prior verification result.

Set `"reuse_verified_presentations": true` in the conform manifest to enable
REEL's local adoption-verification cache. The default remains a fresh full
adoption check. A cold or invalid cache record runs that same full check and
atomically records success under
`<asset-root>/.reel-verification-cache/presentation-adoption-v1/`.

A warm hit rehashes the current adoption manifest, every referenced catalog,
template, source template, scoped binding and selection-evidence file, selected
source/master/receipt, and the REEL validator, FFmpeg and FFprobe executables.
Changing any of these invalidates reuse; corrupt or incomplete records are
misses. Missing or altered selected input bytes are still errors. Cache records
are trusted local build evidence and can be removed to force fresh checks.
The episode receipt reports fresh and reused presentation checks separately.
This option skips repeated adoption rebuilding/decoding only: the new episode
still undergoes ordered picture/audio comparison, timestamp verification and
full compact-output decoding. Scene receipt checks and creative approval remain
independent.

### Consume only a verified native scene master

A semantic-scoped direct scene segment may use the exact
`reel.scene-delivery-receipt.v0.1` as both `source_receipt` and
`delivery_receipt`, with `semantic_delivery` and `delivery_job` pinned. By
default, conform freshly checks the complete scene output directory.

For bounded assembly, first issue a native proof while all scene outputs are
available:

```text
reel-episode-conform prove-scene <delivery.json> --asset-root <assets> --output-dir <complete-scene-output> --proof-path <new-proof.json>
```

This performs the full scene check, including every stem, picture, review,
overlay and master. It writes `reel.scene-master-proof.v1` only on success.
Retain the complete checked outputs and receipt in asset management. Then add
`master_proof` as an exact FileRef to the semantic-scoped scene segment. The
assembly workspace needs only `master.mkv` and `receipt.json` from that output
set, alongside the exact source inputs, job, semantic selection and proof.

Conform replans and rehashes all current scene inputs, rehashes the selected
master and receipt, and requires the same validator, FFmpeg and FFprobe hashes
as the full proof. It still decodes and compares every ordered episode frame
and audio sample. A changed job, input, plan, master, receipt or tool invalidates
the proof. This route is unavailable for unscoped, imported, presentation or
legacy scene-build receipts. It avoids copying unconsumed outputs; it does not
remove them, approve the movie, or replace the standalone full scene check.

### Explicit delivery geometry

By default, every selected source must have the same picture dimensions and
frame rate. To join different resolutions, declare an output size explicitly:

```json
"output_geometry": {
  "width": 1280,
  "height": 720,
  "policy": "preserve-aspect-lanczos"
}
```

This policy accepts square-pixel sources with the same aspect ratio as the
target. It rejects cropping, stretching, non-square pixels, unsupported
policies, and frame-rate changes. Dimensions must be 1–16384. Sources already
at the target size are retained at that size. Other sources are scaled with
Lanczos as part of lossless normalization; the declared transformation is
independently decoded from the original and must match normalized picture
bytes exactly. Original source hashes, receipts, frame counts and audio
checks remain authoritative. Upscaling does not add source detail.

Legacy streams with no usable pixel-aspect tag remain rejected unless the
author explicitly adds `"assume_square_for_unspecified_sar": true` to
`output_geometry`. Only absent/unknown, `N/A`, or `0:1` tags qualify. A known
non-square value still fails. Normalization writes `setsar=1` and independently
checks decoded content against that declared transform; original source bytes
and the original tag stay recorded. The segment receipt adds
`sample_aspect_ratio_assumed_square: true` only when this assumption was used.
Writing the tag alone is not reported as scaling. This declaration does not
resolve inconsistent color metadata or authorize reinterpretation of known SAR.

The original frame and audio timestamps are checked before any episode clock
rewrite. Sources must have consistent color range, matrix, transfer and primaries
metadata; normalization and the conformed master must preserve these values and
square pixel aspect ratio. Unknown color tags remain unknown; this policy does
not infer or invent color calibration.

For legacy streams with an unspecified matrix, the author may explicitly set
`"assume_color_space_for_unspecified": "bt470bg"` in `output_geometry`.
Only this supported declaration is accepted. It does not replace a known matrix
or fill unknown range, primaries, or transfer tags. Before resizing, the conform
independently decodes the original with and without that matrix declaration and
requires identical RGB bytes. A declaration that changes displayed colors fails.
Normalized streams, the full master, and compact output must retain the declared
matrix; the segment receipt preserves original color tags and records the
assumption. This is a verified interpretation of untagged decoded content, not
permission to relabel known color spaces or claim source color calibration.

The receipt records the output policy and, for each segment, original and
output dimensions, original decoded picture hash, whether scaling occurred,
and successful transformed-source comparison. These fields are omitted when
no output geometry is declared, preserving the legacy receipt shape.

A scene segment may also supply `delivery_job` (an exact authoring-root file
reference) and `delivery_receipt` (an exact media-root file reference beside the
hydrated scene outputs). They must be supplied together. The selected scene
master must be that delivery directory's `master.mkv`; the scene-build receipt
must bind the selected delivery job and receipt hashes. The conform then runs REEL's
independent `scene_delivery::check` against the selected job, all delivery
outputs and current assets. Each segment records `upstream_delivery_verified`.
The episode receipt says `verified-for-all-scene-segments` only when every scene
passed this recheck; otherwise it leaves that gate open.

For scenes authored directly through `reel.semantic-delivery.v1`, a scene
segment can instead supply `semantic_delivery`, a hash/byte-bound reference in
the authoring root, and use the original `reel.scene-delivery-receipt.v0.1` as
`source_receipt`. Both `delivery_job` and `delivery_receipt` remain required;
the latter must bind exactly the same receipt bytes as `source_receipt`.
The semantic manifest must explicitly scope the segment's scene ID and language.
REEL resolves its selected graph, checks every semantic attachment, verifies the
selected job's canonical path/hash/bytes and the rendered job/master hashes,
then performs the full scene-delivery media check. A broader graph target is
allowed when its explicitly scoped scene/language events pass semantic planning.
This route accepts ordinary scene segments without a presentation role. It does
not issue a scene-authoring build receipt, skip media validation, or establish
creative approval. Existing scene-authoring receipt behavior is unchanged.

The generic master-order definition has schema `reel.episode-master-template.v1`,
`template_id`, `ordered_roles`, and optional `optional_roles`. `chapter-scenes`
is the one required scene-sequence marker. An optional
`source_template_sha256` pins an upstream project master template, which the
manifest must supply as an exact file reference. The conform verifies the
definition hash against the catalog, expands scene presentation roles and
ordinary chapter scenes, and rejects reordered or missing roles and scene IDs.
The template owns ordering; scene files and episode presentation invocations
own their content and selected bindings.

For optional chapter-closing photographs, use
`reel.episode-master-template.v2` and its `ordered_units`: place each selected
photo presentation directly after its chapter's final scene. Omit the unit
when that episode has no photograph there. Each occurrence has a distinct
presentation ID; its template owns image fit and caption layout, while scoped
bindings select the image and language-local caption. Duration belongs to that
presentation's authored delivery. The conform checks source-text evidence and
selected media just as it does for other presentation units. This also supports
photographs between chapters or immediately before end credits, without an
episode-specific renderer or a global photo toggle.

Each input must decode as one FFV1/yuv444p picture stream and one stereo PCM24
audio stream. Geometry and frame rate must agree. An input at a different
sample rate is explicitly resampled to the selected output rate in a temporary
segment while its decoded picture is verified unchanged and sample count is
checked against the rational conversion. The conform concatenates those
segments without another encode, compares every decoded output frame and audio
sample against the normalized ordered segments, rejects cumulative A/V drift
above one frame, and checks the continuous output timestamps. It writes a
lossless `master.mkv` and a hash-bound `receipt.json` only after validation.
Temporary normalized segments are removed after the completed output is
published.

The receipt reports basic audio-step and black-at-cut findings for each adjacent
segment. These remain review findings; music-tail, ambience perspective,
external VFX/caption provenance, all text transitions, whole-movie viewing,
listening, principal approval and publication are separate gates. The conform
checks source receipt identity and master bytes. With selected scene jobs and
delivery receipts it rechecks their complete REEL scene-delivery outputs. It
does not rerun the authoring compiler, independently rebuild presentation
masters, or prove asset-authority or human approval. Real episode promotion
must retain those upstream gates.
