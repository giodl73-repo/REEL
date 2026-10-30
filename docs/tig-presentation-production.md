# Declarative presentation producer

`presentation_still_sequence` exposes the existing v1 fixed-duration and v2
source-clock still-sequence producer as library code. The existing command is
a thin wrapper; original manifests and receipts keep their schemas and fields.
No legacy project script or new presentation format is required.

`build_with_control_files` accepts an exact manifest pin and separate metadata
and primary-input providers. Template, timing, source clock and clock evidence
use bounded hash-checked metadata reads. Pictures, audio, editable layer and
fonts use exact verified input paths. Media programs come from the input
provider; an explicit refusal has no source-file or PATH fallback. The caller
keeps every guard alive throughout synchronous production.

```text
pinned producer manifest
        |
        +-- control provider: template / timing / clock / evidence
        +-- input provider: pictures / audio / editable layer / fonts
        +-- explicit media programs
        |
original still-sequence kernel
        |
owned candidate directory: master / review / layer / fonts / receipt
        |
check_with_control_files
```

Master and review hashes stream from disk rather than loading entire movies
into memory. The native frame and sample counts, geometry, source-clock spans,
audio treatment and original receipt checks are preserved. The source-clock
integration case compares actual decoded picture and PCM bytes between the
original local path and cache-backed production after original inputs disappear.

`check_retained_with_files` checks exact retained master, review, layer, font and
receipt subjects directly through a separate output provider. Font coverage and
output pins must match the original receipt and manifest. A mismatched movie pin
refuses before opening that movie. Original source and output directories may
both be unavailable; no reconstructed directory view or media copy is required.
Receipts additionally retain their exact episode, language, role and template
identity. Layer and font checks stream instead of reading whole files into RAM.

These low-level APIs do not reserve capacity, enroll Tig jobs, publish frozen
resources, establish complete producer correspondence, grant approval or allow
eviction. Receipt correspondence alone does not independently reconstruct every
picture/audio sample from the primary inputs. Keep that distinction when
admitting a retained result under a newly compiled recipe: never rewrite original
receipt or plan hashes to substitute a new production history.

## Primary-reference comparison without movie duplication

`compare_master_to_primaries` checks one exact master against the shared native
rendering command using stream hashes. It does not require an old receipt or
review file and never rewrites historical evidence to fit a new recipe.
`compare_retained_to_primaries` additionally checks the original retained set and
receipt before that comparison. Both compare YUV444 and displayed RGB24 picture bytes, plus PCM24
audio, with the retained master. Native frame/sample counts and continuous
timestamps are checked separately. A format-correct movie at the wrong cadence
cannot pass by retaining the same frame sequence.

Only the exact declared ASS and fonts are copied into owned temporary scratch.
Their summed logical lengths must fit an explicit caller-supplied staging byte
budget before scratch creation. The renderer emits its movie bytes to the hash
muxer, not another media file. Temporary staging is removed on success or error.
This budget covers those copied bytes; the managed job runner must separately
reserve capacity and bound runtime/resources.

The sealed `PrimaryComparison` returns the exact manifest/master pins and matched
stream digests. It has no public constructor. The future Tig job adapter must
bind this result to its current source, recipe and admitted runtime before
issuing currency or publication capabilities. It is not an original-producer
assertion or a new approval.

The stream parser supports the original
[FFmpeg 8.1 protocol](https://github.com/FFmpeg/FFmpeg/blob/n8.1/libavformat/hashenc.c)
and the version-2 stream-checksum headers emitted by newer tools. It requires
exactly one raw picture stream and one PCM24 audio stream under SHA-256 and
rejects duplicate, missing, wrong-type, wrong-algorithm and unsupported headers.
No version-specific command option is imposed on the older implementation.

The regression fixture deliberately replaces a cel with a same-geometry image
and writes **new forged fixture metadata**, leaving original objects intact.
Receipt/format checking can pass that metadata; the primary-reference comparison
must reject its actual pixels. This demonstrates why metadata checks cannot
substitute for primary-to-output validation.

A separate fixture withdraws the old receipt binding while preserving the exact
master and primary inputs; the master-only comparison still passes. This permits
honest revalidation against a new declared render without pretending that an old
producer emitted a new receipt. Historical provenance and creative approvals
remain separate immutable subjects.

Reference image decoding, raw encoding and filter execution use explicit thread
limits. This limits the verifier's burst concurrency; it is not a replacement
for a managed job's process/runtime reservations.

## Producer checklist findings

Scope: still-sequence extraction and guarded producer inputs. Owner: native
presentation integration. These are producer-applied editing, sound and
rights/provenance checklists, not independent or human approval.

- Editing: preserve fixed and semantic frame spans; verify decoded picture
  parity and reject missing source bindings before creating output.
- Sound: preserve selected audio, native sample clocks and envelope behavior.
  Clock evidence is exact pinned metadata; it does not approve a recording.
- Provenance: retain the distinction between an original picture-only producer
  and later song remux. No receipt hashes are rewritten to fit a new recipe.
- Runtime, high severity before full currency admission: supplied font files
  are pinned and copied, but libass font fallback also depends on runtime
  configuration. Complete presentation interpretation must bind/enforce font
  selection and the runtime environment; explicit program paths alone cannot
  establish complete runtime closure.

Native presentation recipe/currency admission, bounded parent execution and
complete resource-owner handoff remain open.

## Local qualification

All five still-sequence integration cases pass, including source-clock cache
production and decoded parity with originals unavailable (4.35 seconds total).
The library passes 213 tests with nine explicitly ignored (2.61 seconds),
including the timing tests moved with their kernel. Locked all-target,
all-feature Clippy passes with warnings denied. Hosted qualification is pending;
these checks do not qualify actual project adoption or installed runtime pins.

The retained-output extension also passes with the output directory renamed
offline, missing retained subjects and contradictory movie pins. Its final run
passes all five integration cases (15.93 seconds), 213 library tests with nine
explicitly ignored (13.50 seconds), and locked all-target/all-feature Clippy with
warnings denied. Hosted qualification remains pending.

The final primary-reference extension passes all five integration cases (12.16
seconds), including silent/audio-envelope/source-clock geometry cases, cache-only
comparison, missing old receipt, budget refusal and the forged cel metadata
counterexample. Library tests pass 214 with nine explicitly ignored (3.52
seconds); protocol parsing also rejects malformed version/type/algorithm/stream
coverage. Locked all-target/all-feature Clippy passes with warnings denied.
These are fixture results, not a claim that actual project masters have yet
passed primary-reference comparison or native job admission.
