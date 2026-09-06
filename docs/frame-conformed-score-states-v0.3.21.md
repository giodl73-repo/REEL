# Frame-conformed score states v0.3.21

REEL's `score-state` edit mode renders discrete visual states—such as changing
noteheads and lyric highlights—without cinematic transitions or cumulative
duration drift. It is additive; existing `cinematic` and `montage` behavior is
unchanged.

## Contract

- Every shot start and duration must resolve to an integer frame at `--fps`.
- Shots must be contiguous and each state must hold at least one frame.
- Assembly uses hard-cut concatenation and trims the final video by exact frame
  count.
- A supplied master `--audio` requires a passing, hash-matched
  `--audio-check-report`. Audio is padded or trimmed to the conformed timeline,
  so AAC/container tail behavior cannot shorten the picture.
- A completed render must have the exact conformed duration. The artifact
  report records `edit_assembly: frame-conformed-hard-cut`; `animatic-check`
  applies the same zero-drift duration rule.
- `--clean-picture` emits no burned captions, speaker badges, or persistent
  disclosure overlay. This flag controls picture cleanliness only; it does not
  imply review, rights, publication, or release approval.

## Example

At 50 fps, shot boundaries are multiples of 20 milliseconds:

```powershell
reel animatic-render score-states.yaml --asset-root frames `
  --audio original.wav --audio-check-report original.audio-check.json `
  --edit-mode score-state --fps 50 --clean-picture `
  --output golden-songbook.mp4 --format json
```

Run `reel animatic-check golden-songbook.artifacts.json --output json` after
rendering. Use `reel animatic-receipt` and `reel animatic-receipt-check` when a
path-free handoff receipt is needed.

The manifest remains the production contract: use one shot per visual state,
with `start_seconds` and `duration_seconds` conformed to the chosen frame rate.
