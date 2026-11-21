# Bounded private media review

`reel-media-review REQUEST NEW_ABSOLUTE_OUTPUT` freezes explicitly selected local
video, audio or image files for the existing read-only `reel-editor-host`.
Build both binaries with `cargo build -p reel-assets --bin reel-media-review
--bin reel-editor-host`. No server starts during export.

The `reel.media-review.v1` request has `title`, `notice` and one to sixteen `items`.
Each item declares `id`, `label`, `language`, `kind` (`video`, `audio`, `image`),
and `source: { path, sha256, bytes }`. Sources must be absolute regular local
files with matching hashes and supported kind/extension pairs: MP4 video;
WAV/MP3/M4A audio; PNG/JPG/JPEG images. Bounds are 128 MiB per source and 256 MiB
per export. Existing output directories, changed sources and unsupported inputs
are refused. Inputs are checked before a new export directory is created.

The native export has a frozen frontend, path-free session, hashed media routes
and export receipt. The host rechecks bytes, kind/MIME and routes, serves only
declared resources over a capability-protected loopback URL, and supports media
range requests. Export is a review packet, not creative approval or publication.
Start the server explicitly with a finite lifetime when review is requested.
