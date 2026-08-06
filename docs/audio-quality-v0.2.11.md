# Local audio-quality gate in CLI v0.2.11

`audio-check` analyzes audio locally through FFmpeg/ffprobe and emits strict,
path-free `reel.audio-check.v0.1` JSON:

```powershell
reel audio-check narration.wav --profile private-review `
  --manifest manifest.yaml --report narration.audio-check.json --output json

reel audio-check mix.wav --narration-stem narration.wav `
  --effects-music-stem ambience.wav --profile youtube-audiobook --output json
```

Profiles are `audiobook`, `podcast`, `youtube-audiobook`, and `private-review`.
Each serializes its complete thresholds. Measurements include integrated LUFS,
loudness range, true peak, sample peak, the number of samples at the measured
maximum, codec/sample format/bit depth where available, sample rate, channels,
duration, and leading/trailing/internal silence ranges. Supplying both stems
adds their independent hashes/facts and the narration level margin.

`--manifest` compares measured duration with the conformed timeline to 50 ms.
`--report` atomically retains the same JSON and refuses overwrite. JSON never
contains filenames, local paths, transcript text, manuscript references, or
speaker names. Violations contain only a code, measurement, and optional time
range.

A passing retained report can gate and bind a render:

```powershell
reel animatic-render manifest.yaml --asset-root assets --audio narration.wav `
  --audio-check-report narration.audio-check.json --captions captions.srt `
  --output review.mp4
reel animatic-check review.artifacts.json
```

The renderer verifies the report schema/pass state, audio hash, and conformed
duration before FFmpeg. The artifact records a path-free binding with report
hash, profile, and audio hash; `animatic-check` verifies the report input and
binding again. Audio analysis never normalizes, repairs, clones, synthesizes,
selects, uploads, or approves a voice.
