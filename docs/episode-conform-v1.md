# Data-driven episode conform

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
