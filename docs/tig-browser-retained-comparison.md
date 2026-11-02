# Retained browser comparison

`reel-tig-job compare-browser-retained PROFILE NEW_PRIVATE_REPORT` explicitly
replays a frozen browser context and its original declared recipe, holds every
primary input and both retained media files against mutation, checks the native
master format, and runs the existing complete SDR picture/audio/clock comparison.
It does not launch a browser, render media, copy media into a worktree, adopt a
source, change selection, transfer approval or authorize eviction.

The profile schema is `reel.tig-browser-retained-comparison.experimental-1`.
It supplies `metadata_root`, `metadata_minimum`, archived `source`, `context`,
browser `request`, independently admitted original `producer`, `claim`, `receipt`,
and `reference` objects. `byte_source` enrolls retained objects with mandatory
mutation prevention; `runtime` separately enrolls the current checker and media
programs. Existing `cache`, `preparation_cache` and `max_verified_bytes` admit
bounded report preparation and unique retained-object sizes (claim, receipt,
reference and master). This is not a bound on repeated decoder I/O. Primary byte
admission remains in the source profile and request. Both primary and
retained byte providers must require `prevent-mutation`.

The original producer declaration must still be present at its exact metadata
pin. It is never reconstructed from the current checker or relabeled as current
production. Source replay checks the original context, admitted output definitions
and complete declared recipe against the original unsigned claim. This is
correspondence evidence, not independent execution attestation.

The current comparison requires a nonempty audio clock and integral 48 kHz sample
duration. The candidate must satisfy the original FFV1/yuv444p and PCM24 native
contract. The reference may have another supported SDR codec; exact decoded
picture planes, displayed RGB/color metadata, PCM and timestamps must still match.
A lossy historical export may compare unequal; that result must not be converted
into equivalence or approval. Silent-source comparison remains unsupported.

Reports are private and published without overwriting existing files. Stdout and
failure diagnostics contain fixed stages and aggregate results, not private
paths or asset identities. An unequal completed comparison writes its report and
returns a failing exit status; refusal writes no report. Source metadata and job
ownership remain unchanged. The report grants neither current production currency
nor creative approval.

Producer-applied Editor review preserves every frame/sample/clock check rather
than treating matching file size as content equivalence. Rights/Provenance review
keeps original producer identity, current checker identity and human approval
separate. Storage review requires held bytes, existing admitted scratch and
no-clobber reports, with unchanged source metadata. These are checklist findings,
not independent approval or production-lane acceptance.
