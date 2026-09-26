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
