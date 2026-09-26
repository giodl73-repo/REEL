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
