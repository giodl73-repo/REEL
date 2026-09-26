# Data-driven episode conform

`reel-episode-conform` builds one language-local lossless FFV1/PCM24 master from
selected scene and presentation masters:

```text
reel-episode-conform build <manifest.json> --input-root <authoring-root> --asset-root <hydrated-media-root> --output-dir <new-dir>
```

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
with the selected scoped master binding.

The generic master-order definition has schema `reel.episode-master-template.v1`,
`template_id`, `ordered_roles`, and optional `optional_roles`. `chapter-scenes`
is the one required scene-sequence marker. An optional
`source_template_sha256` pins an upstream project master template, which the
manifest must supply as an exact file reference. The conform verifies the
definition hash against the catalog, expands scene presentation roles and
ordinary chapter scenes, and rejects reordered or missing roles and scene IDs.
The template owns ordering; scene files and episode presentation invocations
own their content and selected bindings.

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
checks source receipt identity and master bytes but does not independently
rebuild every upstream scene or presentation receipt. Real episode promotion
must retain those upstream checks and asset-authority evidence.
