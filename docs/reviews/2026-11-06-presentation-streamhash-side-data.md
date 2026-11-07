# Presentation comparison with stream side data

A still source can produce an FFmpeg v2 stream checksum record with more than
three fields. The optional `S=N` suffix contains N size/checksum pairs for coded
side data. The old parser refused that valid record before comparing the decoded
picture and PCM streams.

The parser now validates the complete optional suffix: a positive count, exact
field cardinality, unsigned sizes and SHA-256 digests. It continues to require
the original two stream identities, raw-video/PCM headers, nonempty payloads and
one complete payload digest per stream. Unknown, malformed, duplicate or missing
records refuse. Legacy records retain their existing interpretation.

The comparison contract remains decoded YUV, RGB and PCM equality. Side-data
framing is validated separately from those payload hashes; the exact source
object pins continue to preserve all original source bytes. No source selection,
rendering treatment, capacity policy, producer-current claim or approval changes.

Fixtures cover valid suffixes on either stream and reject bad markers, counts,
overflow, sizes, digests, truncation, duplicate subjects and legacy suffixes.
Error diagnostics describe framing without including private media or paths.

The protocol follows FFmpeg's
[hash muxer implementation](https://github.com/FFmpeg/FFmpeg/blob/master/libavformat/hashenc.c),
where `hash_write_trailer` appends `hash_print_sidedata` for v2 per-stream output.
Producing repositories retain private observations and runtime qualification;
technical comparison is separate from human review and production adoption.
