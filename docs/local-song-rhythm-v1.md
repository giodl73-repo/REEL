# Local song rhythm checks

`tools/local_song_rhythm.py` is an offline, read-only signal diagnostic. Private
inputs and reports belong to their producing asset repository/cache. Run them
through the shared Rust job contract with a hash-pinned tool, installed Python,
source listening files and exact request. No service, model, download or private
catalogue is bundled here. `--batch` accepts up to 128 explicit input/hash/output
entries; each recording is limited to 15 minutes before decoding.

The report retains the exact source and implementation hashes, installed library
versions, method parameters, onset envelopes and twelve-second window estimates.
It measures raw onset autocorrelation rather than scoring the regularity of a
beat grid that a tracker already imposed. It retains alternative tempi and raw
local estimates alongside explicitly octave-folded tempo spread. Long repeating
patterns are reported separately to preserve evidence for syncopation and clave.

Harmonic/percussive separation is an estimate. Detected attack density is not an
instrument count, and eight-second chroma recurrence is a harmonic-pattern proxy,
not recovered melody. A steady pulse can accompany a crowded arrangement. Genre,
vocal activity, tempo ambiguity and song form need listening context.

There is no universal quality score or approval threshold. Producing projects
may compare current Goldens and actual rejected candidates to establish empirical
review bands. A band is advisory until listening-labelled examples demonstrate
that it distinguishes the desired problem without rejecting accepted music.

Synthetic tests exercise jitter, tempo change, repeating syncopation, dense but
steady attacks, gain invariance, silence and harmonic recurrence:

```powershell
python -m unittest discover -s tools -p test_local_song_rhythm.py -v
```

Sources: [onset strength](https://librosa.org/doc/0.10.2/generated/librosa.onset.onset_strength.html),
[local autocorrelation](https://librosa.org/doc/0.10.2/generated/librosa.feature.tempogram.html),
[chroma](https://librosa.org/doc/0.10.2/generated/librosa.feature.chroma_stft.html), and
[recurrence](https://librosa.org/doc/0.10.2/generated/librosa.segment.recurrence_matrix.html).

The producing job can supply `--runtime numeric-runtime.json` to freshly verify
the installed Python and an explicit numerical-library file inventory before
loading audio. The report retains that runtime descriptor. This profile uses
installed numerical libraries only; it requires no music or recognition model.

Spectral-register spread and one-second harmonic energy-distribution changes
add texture diagnostics. These are signal proxies, not instrument counts or
melody transcription. Actual accepted/rejected listening labels remain separate.
