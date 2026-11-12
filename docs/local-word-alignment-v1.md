# Local word alignment

`tools/local_word_alignment_worker.py` runs an explicitly supplied existing
WhisperX environment with exact supplied TorchAudio English or Spanish CTC
models. REEL's existing Rust jobs own disk admission, input staging, process
timeouts and output custody. The worker adds no resolver, production script,
daemon, renderer or download path. Stage `local_tts_worker.py` beside it for the
shared existing path/hash and offline guards.

The `reel.local-word-alignment.v1` request pins each PCM WAV, exact source text,
native sample clock, model file and runtime/package inventory. A batch contains
at most 512 cues, each at most 120 seconds. Native evidence uses
`reel.scene-word-timing-evidence.v1`. Complete lexical agreement, positive word
durations, monotonic starts and the exact take's sample boundary are required;
incomplete measurements remain explicit held diagnostics. Forced alignment
is a measurement of supplied wording, never proof of pronunciation, source
fidelity, human listening or performance selection.

The PCM clock reader accepts ordinary RIFF PCM and WAVE_FORMAT_EXTENSIBLE
with an integer PCM subtype, including under Python 3.11. It validates mono
16/24/32-bit frames without modifying the supplied take. Unsupported subtypes,
truncated chunks and inconsistent clocks fail before model execution.

No audio is uploaded and no model is installed by the worker. Offline flags and
socket refusal prevent fallback downloads; the two model bundle filenames must
already exist and match their declared bytes. Pin the supplied Python,
WhisperX alignment source, package versions and FFmpeg binary. Trusted package
runtime remains the caller's responsibility; this is not an untrusted-code
sandbox. Keep source requests, models and private results in the producing
project's cache/LFS, not this repository.

Run with `python local_word_alignment_worker.py request.json`. A WSL invocation
must use an explicit existing interpreter, inherit the admitted staged directory,
and have a separate Linux timeout; Windows process-tree control alone must not
be claimed to terminate a detached Linux process. Stage temporary model paths
inside the job. No setup or persistent service is part of this workflow.

On Windows, stage `tools/wsl_python_worker.py` and invoke it through the exact
supplied Python executable. Its argument array bridges WSL's option parser
without constructing a shell command. Declare both Python and WSL as job tools,
the Python DLL layout and all worker files as inputs. The launcher requires a
named distribution, an existing absolute Linux interpreter and an inner Linux
timeout. The ordinary Rust runner still owns the outer Windows process tree.

Synthetic checks: `py tools/test_local_word_alignment_worker.py`.
