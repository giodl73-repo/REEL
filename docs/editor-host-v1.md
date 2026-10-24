# Explicit native editor launcher

```text
reel-editor-host serve <export-directory> --open
```

This foreground Rust command hosts one explicitly chosen native editor export
on `127.0.0.1`, with an OS-random private capability URL. `--open` requests the
default browser; the URL is also printed as startup JSON for manual use. Each
invocation is independent. Orientation, export, application and repository
startup never launch it automatically.

The same command supports cel sessions v1–v4, note workshop v1 and lyric-underlay
workshop v1. Export first with the matching native tool. The host checks the
bundled HTML/JS against its built-in frontend revision, allowing CRLF/LF source
line endings; a different frontend requires a fresh matching export. Session
bytes stay exact. Opening an editor does not replace native model validation,
compilation, source-lock checks, asset retention or human review.

Options:

| Option | Behavior |
| --- | --- |
| `--port 0` | Default: choose an available loopback port. An explicit port is also accepted. |
| `--idle-seconds 900` | Stop after inactivity. Only successful resource requests refresh activity. |
| `--max-seconds 1800` | Stop at the lifetime deadline, independently of requests. Time limits accept 1–3600 seconds. |
| `--report <new-private-report.json>` | Reserve a new report and write resource hashes/counts and shutdown state. Existing files refuse. |

Without `--open`, the printed URL can be opened explicitly in a browser. If the
default-browser helper fails to start, manual URL use remains available.
Process termination also closes the listener. Draft downloads and native apply
remain explicit operations; a loaded editor keeps its client-side state, while
reload/media requests need a live host. Save a draft file to carry work between
invocations; a new ephemeral port has separate browser storage.

The listener offers only GET/HEAD over the exact three frontend resources and
declared cel media. It verifies Host, capability path and any Origin header;
there are no listings, arbitrary paths, upload/write/job endpoints or cache
discovery. Unknown routes and encoded/traversing paths reject. Responses use
no-store, same-origin resource policy, no-referrer, framing restrictions and CSP.
Keep capability URLs and reports private.

Control resources are bounded to 2 MiB each and frozen in memory. Optional media
are opened read-only, fingerprinted once under a 1-GiB combined budget and served
through those handles. Image filenames must match their SHA-256; the exported
movie has its own fingerprint. Changed media metadata refuses subsequent reads.
Headers, socket waits and streaming loops are bounded; deadlines are checked
during verification/serving. Up to 16 request readers run concurrently, with
accepted sockets explicitly switched to blocking reads and five-second socket
waits capped by the deadline. Idle browser preconnections therefore do not block
other resources. Media seek positions serialize only within the same asset.
Byte-range and HEAD responses support browser media
access without additional copies. This transport check alone does not establish
image decoding, recording identity, playback quality or rendered acceptance.

Tests cover the parser, resource/identity rules, ranges, changed resources and
real process/socket behavior including idle/lifetime expiry. Private producer
browser evidence belongs to the project cache/LFS; public tests use synthetic
controls and do not open a GUI or contain private media.
