# Existing presentation master intake

`reel-presentation-adopt` turns a selected existing opening, card, caption or
credits master into a lossless, hash-bound segment for episode conform:

```text
reel-presentation-adopt build <manifest.json> --input-root <authoring-root> --asset-root <hydrated-media-root> --output-dir <new-dir>
```

The `reel.presentation-adopt.v1` manifest names one episode, language, role and
selected template. It points to exact catalog/template bytes, season and episode
bindings, the scoped source binding key and exact hydrated source bytes. Optional
legacy selection evidence is an exact JSON file plus a JSON Pointer to the
selected hash or `cache://sha256/` URI. The two evidence fields must appear
together. A source-template reference is required when the selected template
pins an upstream owner template hash.

For repeated uses of one template, set `template_kind` to its reusable kind
(for example, `poem-title`) and give each occurrence a distinct `role` such as
`poem-title-opening` and `poem-title-second-chapter`. Use the same pair in the
episode's presentation invocation. Each occurrence retains its own scoped asset
binding and source-text evidence; geometry stays in the shared template. Omitted
`template_kind` preserves the existing role-as-kind behavior. Empty or mismatched
kinds are rejected. The occurrence role remains pinned in the adoption receipt.

The JSON Pointer proves that the named evidence file contains the selected
hash. When that file is a graph of candidates and selections, the producer must
also inspect its slot disposition and selected revision; the pointer alone does
not interpret a graph's selection semantics.

The generic `reel.selected-presentation-master-template.v1` definition owns
the output width, height, frame rate, sample rate and exact frame count. The
episode manifest therefore selects the template and source without repeating
its layout or duration. This route is appropriate when the production rule
says to reuse an existing selected master, such as a season opening. Fresh
editable credits or other newly authored presentation need a render route.

The builder verifies source hash/bytes and scope, legacy evidence when supplied,
stream geometry and clocks, then converts H.264/AAC or another decodable source
to FFV1/yuv444p and stereo PCM24. It fully decodes both sides into the same raw
picture/audio formats and requires identical bytes and counts. It verifies the
selected frame count and continuous output timestamps before writing a new
`master.mkv` and `reel.presentation-master-receipt.v1` receipt. That receipt
states `technical_validation_state: decoded-source-equivalent` and keeps
creative review and publication open. It does not establish that the inherited
source text, cast, score, or imagery was approved.

When episode conform consumes this receipt, its segment must name the exact
adoption manifest. Conform reruns the adoption and compares selected source,
template, evidence and decoded master content. A receipt without that manifest
cannot claim this route's upstream verification.

## Reuse a presentation excerpt from an existing film

When a presentation exists only inside a selected film, add an optional
`source_range` and bind that exact range to the selection evidence:

```json
{
  "source_range": { "start_frame": 2880, "frame_count": 605 },
  "evidence_range_pointer": "/poem/source_range"
}
```

The source binding and hash evidence still identify the entire original film.
The range pointer must identify an equal `start_frame`/`frame_count` object in
the exact evidence JSON. Both range fields require selection evidence. The
frame count must equal the selected presentation template's duration. The
producer establishes the actual boundaries from the original composition or
verified playback; this facility does not infer them from a filename or title.

REEL decodes from the beginning, selects those frames, and selects audio from
`floor(start_frame * sample_rate * fps_denominator / fps_numerator)` through
the corresponding exclusive end sample. No independent audio shift, retiming,
new performance, crop or picture change is applied. The lossless output must
match the decoded source excerpt byte for byte. Empty, overflowing, stale,
wrong-duration and unavailable ranges fail. The receipt records the range and
its evidence pointer; `check` and downstream conform recheck them.

Bind the adopted output master separately from the original source: container
normalization changes file hashes even when decoded content is identical.
An excerpt retains the original film's cast/text/rights state. Its verified
bytes are technical evidence, not new creative or publication approval.

### Explicit repair of inherited audio timestamps

The default retains source timestamps and rejects discontinuities. When the
producer has diagnosed an inherited audio timestamp fault, an excerpt may opt
into a sample-count clock without changing decoded sample content:

```json
{
  "audio_clock_repair": {
    "policy": "decoded-sample-count",
    "evidence_pointer": "/poem/audio_clock_policy"
  }
}
```

The exact hash-bound selection evidence must contain `"decoded-sample-count"`
at that pointer. This policy requires an evidenced source range. REEL selects
the same frame-derived audio samples, in the same order, then assigns audio
timestamps from sample count (`asetpts=N/SR/TB`). It does not shift, resample,
stretch, pad or replace samples. The output must still match the independently
decoded source picture and PCM bytes. Picture timestamps retain their separate
validation: audio repair cannot hide missing picture frames or gaps. The repair
policy and evidence pointer are recorded in the receipt and rechecked by
`check` and episode conform. Selection of this clock repair does not establish
that the inherited performance, mix or picture/audio synchronization was
creatively approved; review the repaired excerpt in episode context.

### Source picture and audio have different origins

An excerpt normally indexes both streams from zero. When the original source
picture starts after its audio, select an evidenced clock policy instead of
authoring a manual offset:

```json
{
  "audio_range_origin": {
    "policy": "selected-decoded-video-pts",
    "evidence_pointer": "/audio_range_origin_policy"
  }
}
```

The hash-bound selection evidence must contain `"selected-decoded-video-pts"`
at that pointer. REEL probes the actual selected picture frame and finds the
unique decoded audio-frame interval containing that PTS. Its cumulative decoded
sample ordinal and local PTS anchor map the excerpt into the native sample
stream. Earlier audio clock resets outside the excerpt are permitted; ambiguous
coverage at its start and discontinuities within the excerpt fail. Picture
clocks remain continuous from the source start. REEL computes
the native audio sample window with checked rational math. The end boundary is
computed before rounding; fractional sample carry is retained. Missing clocks,
negative windows, overflow, unavailable media, and source timestamp gaps fail.
Rendering and independent decoded-source verification use the same resolved
window. The receipt records the original first audio PTS, selected picture PTS, local
audio anchor PTS and sample ordinal, and sample boundaries; `check`
and episode conform independently resolve them again.

This policy chooses samples. `audio_clock_repair` independently controls output
timestamps; an inherited packet timestamp fault may require both evidenced
policies. A timestamp pass alone does not prove that the correct source window
was selected. Neither policy resamples, rewrites a performance, or authorizes
creative selection or publication. Without either policy, existing behavior
remains unchanged.
