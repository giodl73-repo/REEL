# Compact native episode delivery

Scene delivery profiles may set `still_sequence_encoding: "h264-lossless"`.
The initial still sequence uses libx264 CRF 0, yuv444p, and temporal compression.
Its decoded pixels are lossless. Audio stems remain PCM24. Omitted values retain
the existing FFV1 behavior. ASS, timed effects, and camera composition continue
to use the existing lossless compositor; their final scene intermediates may
therefore still be FFV1. The strict scene check verifies the final codec,
geometry, frames, samples, output hashes and selected layer behavior.

An episode conform manifest may include:

```json
"compact_delivery": {
  "crf": 18,
  "audio_bitrate_kbps": 320,
  "retain_lossless_master": false,
  "intermediate_video_encoding": "h264-lossless"
}
```

Conform normalizes mixed FFV1/H264 lossless scenes to the chosen intermediate
codec and episode sample rate. It compares decoded pictures and same-rate audio
against each original source, then checks the ordered full conform and boundaries.
Only after these checks does it encode H264/AAC `episode.mp4`, fully decode it,
check its geometry, frame rate/count, sample rate and audio-duration tolerance,
and record exact output hashes and bytes in `receipt.json`. With retention false,
the new conform's `master.mkv` is removed before publishing the output directory.
Source assets and scene files are never deleted by conform.

Native scene-build receipts bind the exact selected delivery job and successful
full scene-check receipt. Conform rehashes every source/output and verifies the
exact output set and plan before avoiding a duplicate scene-level decode. The
complete ordered episode still undergoes decoded source comparison. This is a
trusted producer proof chain, not a signature against an actor rewriting all
upstream manifests and proofs. Standalone scene `check` always decodes media.

Chapter title receipts with native cues carry the scene's full sample clock;
their ASS display stays template-limited. Heading source scopes remain separate
from prose cue scopes. Overlay visibility checks sample actual ASS intervals,
so a short opening title is checked while it is visible.

Retain original lossless cues, clean cels, editable metadata, selected manifests,
receipts and the compact episode. Temporary scene movies can be removed by the
production owner after final delivery and rebuild provenance are verified.
Technical checks do not establish creative approval or publication permission.

When independently rendered scenes round their pictures to whole frames but
retain exact native audio lengths, an episode can explicitly select
`audio_frame_conform: "pad-silence-to-picture-boundaries"`. Conform calculates
each target from cumulative frame boundaries with integer sample rounding.
It appends at most one frame of zero PCM per segment, rejects any required
trimming, and verifies the exact original audio prefix followed by zeros.
`audio_padding_samples` records the adjustment in each segment receipt.
The final lossless conform regenerates continuous frame/sample timestamps;
ordered decoded picture and padded audio comparison still applies. This policy
is separate from an authored dramatic breath or any narration timing change.
