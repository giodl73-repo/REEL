# Retained H.264 presentation references

An actual retained master had the expected geometry, square pixels, frame count,
4:4:4 pictures and PCM24 audio, but the independent primary-reference comparison
refused its H.264 codec before content comparison. The source remains unchanged;
converting it to another container merely to satisfy this check would create
unnecessary media and change its byte identity.

The private stream contract now distinguishes newly produced FFV1 outputs from
retained primary-reference candidates. Only the latter additionally admits
H.264; all pixel-format, geometry, stream, clock and sample requirements remain.
Successful reference comparison still requires the full YUV, displayed RGB and
PCM hashes to match the current primary render. No quality label or old receipt
substitutes for that comparison. Production receipt checking remains FFV1-only.

Producer-applied animation/editor, sound and provenance review treats accepting
changed pixels, trimming audio or attributing an old output to a new producer
as high-severity faults. Shared REEL tooling owns the implementation; disposition
is complete decoded equality, strict production checks and separate provenance,
approval and adoption. The regression covers an exact H.264 transcode plus
late-picture, audio and lossy-picture changes. Qualification and actual private
reference acceptance are recorded separately from this design review.

Local validation passes the stream-contract unit check, all six presentation
integration fixtures and warnings-denied all-target/all-feature Clippy. An actual
retained H.264 presentation also passes complete YUV/RGB/PCM comparison using the
local candidate and native asset controls. Temporary layer/font staging stayed
below 1.6 MiB, with no movie or cache copy. This technical observation authenticates
no original producer and changes no selection or approval. Hosted qualification
and runtime adoption remain separate.
