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

These low-level APIs do not reserve capacity, enroll Tig jobs, publish frozen
resources, establish complete producer correspondence, grant approval or allow
eviction. The checker currently reads an owned candidate output directory;
direct independently retained output-provider checking remains separate work.

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

Native presentation recipe/currency admission, retained output-provider checks,
bounded parent execution and complete resource-owner handoff remain open.

## Local qualification

All five still-sequence integration cases pass, including source-clock cache
production and decoded parity with originals unavailable (4.35 seconds total).
The library passes 213 tests with nine explicitly ignored (2.61 seconds),
including the timing tests moved with their kernel. Locked all-target,
all-feature Clippy passes with warnings denied. Hosted qualification is pending;
these checks do not qualify actual project adoption or installed runtime pins.
