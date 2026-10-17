# One light impulse across picture cuts

`reel-light-impulse` renders separately bindable picture-derived VFX assets from a single declarative clock. It does not author or replace scene narration, sound or pictures. Use the existing scene authoring/compiler and `timed-video-overlay` bindings to consume its results.

```text
reel-light-impulse validate impulse.json
reel-light-impulse build impulse.json <cache-root> <new-output-dir>
```

The manifest schema is `reel.light-impulse.v1`. It names one `event_id`, language, semantic `anchor_cue_id`, measured `onset_sample`, integer sample/frame rates and output dimensions. Resolve the semantic word or phrase through the language's selected take alignment before authoring this manifest; this tool does not guess word times.

`segments` are ordered contiguous sample spans starting at zero. Each has an `id`, `start_sample`, `end_sample` and source `{sha256, bytes, cache_uri}`. Sources resolve only from `objects/sha256/<prefix>/<hash>` beneath the supplied cache root and are verified before rendering. All sources remain unchanged.

The `envelope` contains:

- `peak` (0–1 brightness addition), `hold_seconds`, `decay_per_second`, `end_seconds`.
- `recoil_x_pixels`, `recoil_y_pixels`, `recoil_decay_per_second`, `recoil_end_seconds`.

Brightness holds once at onset, decays exponentially, and tapers to zero during the last second (or the remaining shorter interval). Recoil is one damped impulse. Edge replication avoids black borders when the picture moves. No arbitrary filter expressions are accepted.

Each segment uses cumulative **ceil** frame boundaries, matching REEL's timed video overlay spans. Its filter evaluates from that shared frame clock, not from a new local onset. Thus a cut between frames neither adds a frame nor restarts or rewinds the light. Segments with no delivered frame are rejected. Use one manifest per language, with its own measured onset and cut samples.

Outputs are lossless FFV1 `yuva444p` picture-derived movies. They carry opaque alpha and must be bound only to their corresponding picture spans. The receipt records original picture hashes, sample spans, delivered frame spans, exact filters and output hashes/bytes. Each output is fully decoded and its dimensions, format and decoded frame count checked. Existing output directories are refused, including partial failed jobs.

Cache registration, native scene consumption, narration/effect mix, visual review and human selection remain separate gates. A successful effect receipt does not grant creative approval. Tests include an actual two-picture render and a non-frame-aligned cut regression discovered by independent review.
