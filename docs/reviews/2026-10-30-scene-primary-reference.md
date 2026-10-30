# Retained scene masters versus current primary inputs

A legacy movie can be the correct selected master without a native Tig producer
claim. Its old receipt is historical evidence, not a substitute for checking the
current pixels and audio. Rewriting the receipt's plan cannot establish that
correspondence.

`scene_primary_reference::compare_master_to_primaries` accepts exact current
job/production controls, guarded primary and selected-output providers, admitted
scratch and an explicit budget. It uses the same native picture filter builder,
ASS/timed-overlay filters, camera expressions and mixer as ordinary scene
rendering. The ordinary renderer retains its existing output pipeline.

```text
current job + contract + production + primaries
                   |
          native picture filters
                   |
          lossless codec pipe
                   |
         overlay/camera pipes
                   |
       YUV + RGB stream checksums  <--- decode selected master

current cues + gains + timing ---> native D/M/E mixer
                   |                       |
             bounded WAV staging     PCM stream checksum
                                           |
                                 compare selected PCM
```

Intermediate video passes through bounded operating-system pipes with
backpressure. No picture, master or review movie is written. Each stage retains
the native lossless codec and pixel format, including quantization between
stages. Timed overlays read their guarded source directly instead of copying it.
Both YUV and displayed RGB are hashed so a display-color change cannot hide
behind unchanged YUV bytes.

Only ASS/font copies and temporary native audio WAVs use disk. A conservative
reservation accounts for float/PCM samples, headers and possible copied source
metadata. Admission rejects insufficient staging or picture-process budgets
before creating temporary scratch. Picture decoders, encoders and filters use
one thread each. The caller chooses the maximum pipeline process count.
Normal completion and refusal remove the operation's owned temporary directory.

The selected master must have the current dimensions, square pixels, frame rate,
stereo sample rate and exact frame/sample counts. Rational decoded timestamps
start at zero and remain on their frame/sample clocks within container tick
rounding. Hash parsing supports both pinned FFmpeg 8.1 stream checksums and the
newer versioned protocol; stream coverage, media types and SHA-256 must match.

Owned pipe processes have a deadline, bounded diagnostics and explicit cleanup;
only their own handles are killed. Production must run this low-level primitive
inside the existing bounded worker, which contains the native mixer and every
subprocess and admits real capacity. A byte estimate is not a disk reservation.
The result is a technical observation, not original-producer identity, creative
approval, independent custody or eviction authority. It does not itself adopt a
Tig job or switch a production binding.

Producer-applied editing, sound and provenance checklists require exact native
timing, all picture/audio samples, unchanged original receipts and no private
payloads in reports. The synthetic test verifies a retained movie with its
receipt absent and rejects changed primary pixels despite a forged matching
receipt. Additional cases exercise ASS, timed overlays, lossless H.264 and camera
motion. Pipeline tests cover checksum protocols, owned-child timeout and bounded
diagnostics. Final exact-source qualification remains required before adoption.

Local qualification passes strict locked all-target/all-feature Clippy, 222
library tests (nine explicitly ignored), and all 27 enabled scene-delivery tests
(14 explicitly ignored). The explicit focal/pan/reduced-motion case also passes
in 99.21 seconds. Final targeted checks reject changed cue audio and shifted
timestamps (6.55 seconds) and verify process-budget refusal on the camera path
(9.70 seconds). The default-feature library build and formatting checks pass.
These are synthetic source/renderer checks, not actual project adoption.

Next integration work: current-source-owned Tig scene proof, complete child
resource handoff, bounded CLI/episode execution and actual legacy acceptance.
These remain explicit cutover requirements rather than inferred success from a
standalone comparison.
