# Native continuous album mastering

`reel-album plan REQUEST`, `render REQUEST NEW_DIRECTORY`, and
`check REQUEST DIRECTORY` implement private, complete-recording album packets in
Rust. This is a file-level producer usable by a declared managed job; it performs
no Tig discovery, publication, selection or provider operation. Materialize exact
Tig-selected inputs through the project's existing admitted job contract first.

Requests use `reel.music-album-master.v1`. Declare the title/delivery stem, exact
FFmpeg executable path/hash/length, private-only permissions, ordered tracks
(`asset_id`, immutable `revision`, `title`, source path/hash/length), integer
`gap_frames`, target LUFS and source true-peak ceiling. The sample standard is
48-kHz stereo. A source may use an absolute retained path or a normal relative
path beneath the request's directory, supporting managed materialized inputs.
Parent traversal and relative symlink escapes refuse. The executable must be
an absolute pinned path. Sources are restricted to WAV, FLAC and MP3; playlist and network
protocols are excluded. A relative packet directory is also bound beneath the
request directory, with traversal and symlink-escape refusal. This supports the
ordinary managed job's `render request.json render` invocation. Absolute packet
paths remain explicit caller choices. Tool and source handles remain open through the operation;
Windows handles deny write/delete sharing. This does not claim a complete DLL or
host environment; managed callers retain their normal runtime/source protections.

Limits explicitly declare maximum input bytes, track frames, total frames, packet bytes and
timeout seconds. The planner validates source identities without launching media
tools. Its conservative estimate includes PCM scratch, lossless/listening masters
and metadata. The renderer requires a new directory. Failure preserves bounded
partials for the caller's recovery workflow; it never removes an existing packet.
Each child has a deadline and is killed/reaped before source guards are released.

Each complete recording is decoded to float32 and measured independently. One
static gain is the lesser of target-LUFS gain and the declared true-peak headroom.
The native quantizer uses float64 multiplication, ties-to-even 24-bit rounding,
and rejects nonfinite/clipping samples. The join has exact integer zero gaps and
no final gap. There is no trimming, retiming, compression or crossfade.

Outputs are `master.flac`, 320-kbps `master.mp3`, chapter metadata, CUE, tracklist,
and `receipt.json`. CUE points to the packet's actual `master.flac`. Exact intermediate PCM scratch is removed only after successful
verification. FLAC must decode to precisely the recomputed complete PCM sequence.
MP3 must fully decode within 50 ms of the declared clock. Its demuxed audio-packet
hash must equal a fresh encode of the verified lossless PCM with the pinned
encoder/settings; changing the receipt hash cannot bless substituted audio.
Packet hashing streams through FFmpeg and creates no second master. Both masters must remain
at or below -1 dBTP. The checker recomputes source measurements, quantization,
track hashes/order and joins independently instead of trusting receipt hashes.
Tampered outputs, changed sources, false gains and overwrite attempts refuse.

These are private engineering receipts: store them in the asset system, never as
public metadata. Technical checks grant no listening approval, master selection,
consent or release. The actual project must retain its source authority, human
decisions, registered outputs and delivery evidence separately.

The synthetic Rust fixture uses two distinct five-second recordings, 44.1-to-48
kHz conversion, late tail pulses, a two-second gap, accented labels, independent
recheck, false-gain/output-tamper refusals and a timed-out FFmpeg child. Set
`REEL_TEST_FFMPEG` to an explicit absolute executable and run
`cargo test --lib music_album::tests -- --nocapture`. An absent environment value
skips that media test; do not call such a run media qualification.

Producer-applied .roles checks: music reconstruction requires full source/PCM
identity and independent reassembly; sound design preserves tails and explicit
gaps/gain; editing preserves declared order and complete recordings; rights and
provenance require private scope and preserve human authority. Candidate tests
and receipts do not constitute independent review or installed-tool adoption.
