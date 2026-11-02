# Large native picture controls

The real managed video pilot reached Windows error 206 when the native picture
renderer passed hundreds of input paths and a complex filter through one command
line. The failed job retained its diagnostic and retired its staging directory.
Its outputs were never approved or selected.

Large picture commands now use a bounded temporary lavfi graph file in the
already admitted workspace. Movie sources name only paths returned by the held
input provider; no media is copied, linked or searched for. Paths escape both
FFmpeg parsers, source protocols allow only files, and decoding uses one thread
per source. The graph file is held through execution and removed before result
publication. Short commands retain their existing route.

The file route uses the declared rational frame clock after concatenation so
single-frame allocations cannot collapse to equal timestamps. Format capability
inspection uses the same admitted FFmpeg executable. Where color-link negotiation
is supported, its unspecified color-space/range sink matches the ordinary CLI
encoder; inheriting RGB range at a YUV sink changes pixels and is refused by the
comparison fixture. No source schema or eligibility policy changes.

The actual retry then reached the timed-overlay boundary check. Automatic overlay
format negotiation converted the already encoded base through RGB, changing some
unselected gradient frames even with lossless encoding. Timed composition now
keeps the declared YUV444 working format. A native H264-lossless/RGBA regression
checks every outside frame in both RGB and YUV and requires visible composition
inside the span. Existing adjacent-overlay terminal-frame checks still pass.

The same control lifetime is used by fresh primary-reference pipelines, which
still write no reference movie. Native fixtures compare complete frame hashes
and timestamps against inline rendering at 25, 24 and 30000/1001 fps, including
offset video and filenames containing filter punctuation. A 236-picture case
exceeds the Windows inline limit, checks every frame's order and clock, and
checks control cleanup without copied inputs.

Producer provenance review keeps technical equality separate from production,
selection and approval. Private physical controls stay in admitted scratch;
current and prior selected assets, active runtime bindings and eviction policy
remain unchanged. Actual project retry and hosted qualification remain separate
from these synthetic fixtures.
