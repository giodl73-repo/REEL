# Local speech worker

`tools/local_tts_worker.py` is an optional offline ML worker for the existing
`reel-assets run-job` contract. The Rust runner still owns admission, private
staging, executable/input hashes, process-tree timeout, logs and atomic cache
publication. This worker adds no cache resolver, job grammar or renderer.

An owner supplies an existing compatible Python/Chatterbox installation and an
explicit private local-use decision. Pin its runtime/package inventory in the
producing project's evidence. Stage this worker, one reference clip, the
authorization decision and every exact local model file as declared inputs.
No setup, model download, remote service, new voice clone, or selection is part
of the worker. Use `from_local`, never `from_pretrained`. Model watermarking is
left enabled. The caller remains responsible for verifying the decision's scope;
the request flags do not create consent.

The bounded request uses `reel.local-tts.v1`. See the synthetic fixture in
`tools/test_local_tts_worker.py` for all fields. Each input descriptor requires a
workspace-relative path, SHA-256 and bytes. Model filenames retain the names
expected by Chatterbox multilingual. The output is mono PCM16 WAV at the declared
sample rate plus a hash-bound technical receipt. Only attenuation needed to
avoid clipping is applied; no pitch or tempo changes occur. Empty, nonfinite or
over-duration results fail without publishing a WAV. Existing outputs are refused.

Run the worker with the supplied interpreter and the staged relative request:

```text
python tools/local_tts_worker.py request.json
```

Within an admitted Windows job, stage the exact Python executable and its core
DLLs together under `tools/`. A declared `tools/python312._pth` control file can
name the explicitly supplied existing standard library, extension directory and
compatible environment's site-packages, followed by `import site`. Declare
`tools/python.exe` as the tool and use `-I worker.py request.json` as its arguments.
This uses the runner's ordinary executable argument contract without shell
quoting. First test that runtime layout with a small synthetic admitted job.
Keep machine-specific paths and private requests in the producer's cache-backed
job, never this repository. Pin the runtime files and package inventory; this
layout is not environment installation or isolation from trusted packages.
Declare both WAV and receipt outputs. The job budget must cover private model
copies, runtime files, scratch and output publication.

The worker sets offline flags and refuses socket connections before importing
the model backend. This prevents accidental downloads; it is not a sandbox for
untrusted Python packages. All jobs and tools remain trusted explicit inputs.

Synthetic checks need only standard Python:

```text
py tools/test_local_tts_worker.py
```

These fixtures verify custody, validation, offline refusal and waveform structure.
They do not demonstrate intelligibility, likeness or human performance approval.
