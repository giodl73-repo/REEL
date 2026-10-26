# Native decoded media comparison

`media_parity::compare_with_inputs` compares two exact file references through
borrowed `scene_delivery::InputFiles` providers. A container hash can change while
its decoded content stays identical. The report keeps those concepts separate:

```text
reference ID + byte pin -- provider --> retained master --+
                                                        +--> native shape/clocks
candidate ID + byte pin -- provider --> new master ------+
                                                        +--> YUV444P frame hashes
                                                        +--> displayed RGB24 hashes
                                                        +--> exact stereo PCM24
```

The explicit shape contains dimensions, rational frame rate, frame count,
sample rate and sample count. Both files must contain exactly one video stream
and one stereo audio stream, start at zero, have square pixels and match that
shape. This profile admits 8-bit SDR and integer audio up to PCM24 precision;
HDR, higher-depth picture/audio and floating-point audio refuse. There is no
scale, frame interpolation, channel conversion, rate conversion or padding.

For every video frame, FFmpeg retains the demuxer's time base. The decoded PTS
must match its native frame-grid position rounded to that container tick, with
the declared frame duration and decoded byte size. YUV444P and RGB24 are compared
separately: equal YUV planes cannot hide a color-space tag that changes displayed
RGB. Color range, matrix, transfer, primaries and chroma-location declarations
must also match: decoding to RGB24 alone does not apply every display transfer
function. A tag-only transfer/primaries change cannot pass merely because its
decoded signal bytes are equal. SHA-256 frame digests are hashed in order, with a newline after each digest.
These sequence digests are not presented as hashes of concatenated raw pixels.
PCM24 is compared as the complete decoded byte stream, with exact sample count;
decoded audio packets must follow the contiguous native sample clock as well.

The decoder streams at most 1 KiB of framehash metadata per record. PCM hashing
uses the existing bounded-buffer segment decoder. Raw decoded pictures/audio
are not written to disk or accumulated in memory. Short/extra content, timestamp
gaps and incompatible stream facts refuse. A valid comparison returns separate
picture, displayed-color and PCM equality fields plus their combined result.
Nothing in the report transfers a selection or human approval.

## Explicit Tig use

The optional `reel-tig-scene` command can compare a freshly checked scene render
with a retained output resolved through Tig:

```powershell
reel-tig-scene --profile .local/tig.json compare .local/scene/delivery/job.json .local/render .local/reference.json
```

The reference file is bounded strict JSON, for example:

```json
{"path":"scene:delivery.es.master","sha256":"<exact-sha256>","bytes":123456}
```

The source profile must explicitly admit that output definition and selected or
named-stream selector. This is comparison evidence, never a production input
edge. The command holds Tig input/reference guards, independently rechecks the
candidate against its job/receipt and derives the comparison shape from that
checked plan. It prints a private result and exits nonzero when content differs.
Physical cache locations are omitted. Keep reference files and digest reports
private; they are not sanitized public receipts.

The candidate is a producer-owned local output directory. Its local Path
provider preserves existing hash checks and adds no mutation protection or job
custody. Generic callers can instead supply protected providers for both sides.
Full source/job/process recovery, candidate custody, approval and publication
remain separate obligations. A successful comparison proves the declared native
content/clock contract, not that a whole project has migrated or is authorized
for release.

Synthetic FFmpeg tests prove container-byte changes with identical content,
picture-only and audio-only changes, equal YUV with changed displayed RGB,
truncation, internal video/audio timestamp gaps, stale pins and sample-rate
mismatch. The real Tig CLI fixture compares a held retained reference, then
detects a cel-only replacement while preserving PCM equality.
