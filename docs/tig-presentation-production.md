# Declarative presentation producer

`presentation_still_sequence` exposes the existing v1 fixed-duration and v2
source-clock still-sequence producer as library code. The existing command is
a thin wrapper; original manifests and receipts keep their schemas and fields.
No legacy project script or new presentation format is required.

## Native Tig source and recipe

`tig_presentation` compiles a small source record whose `authored.document`
names pictures, audio, editable layer, fonts and control records by logical Tig
ID. Every primary pin resolves from one captured source view. Template, timing
and source clock are literal documents on separately versioned records. Evidence
can be a literal document or a small reference to its original selected report;
the compiler supplies canonical control JSON directly in memory. No source
directory, second caller-maintained hash inventory or copied media is needed.

```text
source.tig ── template.tig
    │       timing.tig ── clock.tig ── evidence.tig
    │                          └──── cue.tig (measured object pinned)
    └── cel.tig / cue.tig / layer.tig / font.tig
                        │
            captured metadata + logical primary IDs
                        │
               compiled native manifest
                        │
            frozen Production graph and recipe
                        │
         guarded Tig primary reader + native producer
```

Source/control and primary definitions must be disjoint. The request explicitly
admits primary definition IDs and selection-state values; no universal creative
status is built into the schema. An output definition cannot enter as a primary.
A source clock names both its logical recording and the exact object measured:
changing the selected recording requires a new measured clock before execution.

`requirements`, `freeze_context`, `restore_context` and `recipe` validate the
same compiler interpretation and declared Production graph. A recipe consumes
literal documents and primary payloads, excluding review annotations. A changed
cel, template, timing or recording changes consumed meaning; an annotation-only
commit can retain the recipe while still creating a distinct source checkpoint.
Frozen contexts can be replayed without Git or working control files. A different
request, source, compiler or kernel is refused. All these operations read zero
primary bytes.

The synthetic integration test renders through the actual guarded Tig reader,
checks the generated receipt and independently compares the master to its
primary inputs. Metadata never needs filesystem materialization. This is a
producer test, not yet a managed presentation job, parent-composition admission,
complete runtime/font isolation, creative approval or permission to evict inputs.

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

The native presentation source compiler and metadata recipe are implemented.
Parent presentation currency admission, bounded parent execution and complete
resource-owner handoff remain open. Producer-applied checklist review of this
extension preserves these findings:

- Editing: stable logical cel IDs resolve their selected objects in one source
  snapshot; image and timing changes invalidate the consumed recipe.
- Sound: a recording change cannot reuse a measured clock for another object;
  actual decoding/sample checks remain mandatory during execution.
- Provenance: inline clock evidence is retained as metadata, never treated as
  independent approval. Recipe matching does not change creative status or
  rewrite an old producer's receipt.
- Runtime: in-memory controls preserve explicit provider refusals and remain
  pin-checked. Font fallback and complete runtime isolation remain unresolved
  prerequisites for full reproducibility; the compiler adds no such claim.

These are producer-applied checklist findings, not independent or human review.

## Managed verification of a selected master

A reusable source can have a different scope from its episode occurrence. For
example, an opening produced as `SEASON-OPENING / opening-source` can occur as
`episode-10 / series-opening`. The output record may explicitly declare
`presentation_scope: {"episode_id":"SEASON-OPENING","role":"opening-source"}`.
Those fields must match the referenced native source exactly; its ordinary
`episode_id` and `source_segment_id` retain the occurrence identity. Language
and source ID still match exactly. Without the explicit scope, the original
same-scope contract applies. This does not rename historical producer metadata.
Two occurrences share the consumed native recipe but retain separate selected
output-record and frozen job identities.

With `tig-publication`, `tig_presentation::jobs::JobPlan` binds the native recipe
to a held runtime and one explicitly admitted selected output. The output record
must name its source presentation, episode, language and segment role, with a
separate output definition. Wrong scope or a primary/control definition refuses
before opening media. Selection is project data, not inferred creative approval.

The frozen job retains the complete declared primary set, source interpretation,
runtime, canonical generated controls and the target output record. The selected
master is a **Composition** dependency, never a Primary. Caller-supplied logical
byte/object limits are checked before source adoption. These describe retained
obligations, not allocated storage or a render capacity reservation.

```text
captured source + native recipe + bound runtime + selected master
       │
freeze job → finish/reopen metadata → exact readback
       │
budget preflight → adopt one source obligation
       │
guard primary bytes AND selected master through Tig
       │
fresh primary-reference comparison; check ownership again
       │
VerifiedPresentation owns byte guards and borrows source/runtime guards
```

Ownership reuses the existing per-source fence and journal. Independent source
identities can proceed together; there is no shared-cache/render lock. Resume
requires the exact current owner, revision and frozen selection. Dropping a
guard releases only its local fence and byte handles, preserving the durable
obligation for explicit reconciliation.

`verify_selected_master` obtains both primary and output handles from the same
captured Tig source. It does not accept an unguarded output path or a replacement
master pin. The sealed proof retains those guards through use and refuses a
withdrawn owner. Original receipts need not exist, and no historical production
or approval evidence is rewritten.

This low-level operation must run inside the bounded scheduler in production.
It does not itself reserve capacity, provide a CLI, publish/adopt an output,
establish complete font/environment isolation or retire child obligations.
Parent composition integration and actual-project acceptance remain required
before consumer cutover. The source repository or installed bindings do not
change as a side effect of compiling, freezing or checking these records.

Producer-applied checklist findings for managed verification: the retained
master is an output dependency, never a primary; per-source ownership cannot
become a global cache lock; both media sides retain byte guards; technical
correspondence never rewrites original production or approval evidence. The
master format check also requires exactly two streams, YUV444 picture format
and square pixels, in addition to existing native clocks and sample checks.

## Intake of retained native plans

```text
reel-tig-register --presentation-source PROFILE.json NEW-DIRECTORY

retained v1/v2 manifest object + exact control objects
                 │
       explicit stable asset IDs and filenames
                 │
        bound candidate .tig records
                 │
       Git review/commit → capture Tig view
                 │
      native compile → managed verification
```

`tig_presentation_registration` reuses the bounded registration provider and
writer. It opens only the manifest, template, and optional timing/clock/evidence
objects, each at most 4 MiB and at most 16 MiB in total. It never opens picture,
audio, layer or font payloads. Those pins come from the original native plan,
not a caller-maintained duplicate hash inventory. Candidate records are bound
to exact supplied definitions and schemas before the new directory is created.
Existing output directories are refused. No Git operation, cache publication,
selection change, job adoption or media generation occurs.

The profile schema is `reel.tig-presentation-registration.experimental-1`:

| Field | Contract |
| --- | --- |
| `manifest` | Exact original native manifest object: SHA-256 and byte length. |
| `byte_source` | Existing named cache places and bounded read policy. |
| `definitions` | Exactly `source`, `template`, `timing`, `clock`, `evidence`, `primary`; each supplies absolute hash-bound `definition` and `authored_schema` FileRefs. Definition IDs differ. |
| `nodes` | Exactly the five control roles, each with explicit `id` and portable `.tig` `filename`. Unused v1 timing roles emit no record. |
| `bindings` | Exactly the used `picture/0..N`, `layer`, optional `audio`, and `font/0..N` keys. Each names a stable ID. New records require a portable filename and project-authored fields. |
| `primary_selection_state` | Explicit project data, retained on new primaries; never a grant of creative approval. |
| `existing_source` | Optional exact committed Tig input profile for existing identity reuse/collision checks. No payloads are read from it. |
| `existing_primary_definitions` | Explicit admitted primary definition IDs for reused bindings, disjoint from controls. |
| `extra_audio` | Optional exact audio object for a **new** v1 desired contract based on an original picture-only plan. It cannot replace an existing recording or change a v2 source clock. |
| `metadata` | Optional project metadata; cannot override the registration provenance member. |
| `max_output_bytes` | Explicit total candidate budget, at most 16 MiB. Every emitted record is additionally limited to 64 KiB. |

For example, one binding may be:

```json
"picture/0": {
  "id": "show/opening/cel-a",
  "filename": "cel-a.tig",
  "authored": {"media_kind": "picture"}
}
```

An existing binding instead uses `{"id":"show/opening/cel-a","existing":true}`,
with no replacement filename/authored fields. Its admitted type, selected object
and selection state must match. Identity collision checks use the entire exact
captured metadata namespace, including records outside the selected type
admissions. An unadmitted existing record cannot be silently shadowed.
Shared new IDs require identical selected objects, authored fields and filenames.
IDs are explicit; filenames and hashes never manufacture asset identities.

Definitions control the project-authored fields. Large inventories, receipts
and reports stay assets; they must not be embedded as oversized literal records.
Clock measurement evidence remains its exact original selected byte asset. Its
small authored document uses `reel.presentation-clock-evidence-reference.v1`
with the matching original object pin. The compiler rejects a disagreement
between that pin and the record's selected object. The report is not parsed into
Git metadata: decimal measurements and large report bodies stay in byte storage.
This association preserves provenance; it does not grant approval or prove
independent archival custody.
The original manifest object and any explicit audio extension are retained as
provenance. Historical producer receipts and approval subjects are not rewritten.
A new v1 audio contract still needs independent full master comparison before
reuse. Registration does not assert that its bytes exist or match that output.

The result includes a small `source_request` for the native compiler. Definition
files remain separate reusable repository data; this command emits instance
records. Intake lives outside the fingerprinted compiler/input-adapter modules.
The compiler's evidence-reference interpretation is a separate semantic change
and therefore intentionally changes its fingerprint.

## Explicit presentation inspection and comparison

`reel-tig-job inspect-presentation PROFILE.json` compiles a committed native
presentation source and reports its checkpoint, manifest pin and dependency
counts. It opens no primary media and needs no runtime or shared-store setup.

`reel-tig-job compare-presentation PROFILE.json` independently compares the
selected output with that source's complete expected decoded pictures and PCM.
Its profile schema is `reel.tig-presentation-check-profile.experimental-1`:

- `source`: exact committed `reel.tig-input-profile.experimental-1` profile;
- `request`: native presentation compiler request;
- `comparison`: null for inspection, or an explicit object containing
  `output_id`, `output_definition`, exact `runtime` bindings, existing absolute
  `stage_parent`, `max_staged_bytes`, `max_objects` and `max_logical_bytes`.

Set the source's record scopes to the complete work unit being checked, including
all shared dependencies and the selected output. Freezing archives every record
in those explicit scopes, not just records reached by the render recipe. A
whole-project namespace may therefore exceed bounded metadata work even when a
presentation is small. Validate the full repository namespace separately; missing
dependencies in the scoped production view must still fail resolution.

Comparison binds the output's type, source, language and occurrence through the
same job-plan factory. It freezes and reads back metadata in memory, checks the
declared resource budget, and holds verified primary, selected-output and runtime
guards through comparison. It accepts no caller substitute for the selected
master's hash. Only bounded editable-layer/font bytes enter owned temporary
scratch; the selected movie and caches are not copied or changed.

These commands are synchronous acceptance tools. They do not create shared
stores, reserve scheduler capacity, acquire a global cache lock, adopt durable
jobs, publish results, establish production currency or authorize eviction.
Production execution still requires the bounded scheduler and managed ownership
path. A matching observation keeps original production history and human
selection separate, and is not a complete environment reproducibility claim.

## Local qualification

The intake extension passes all 16 presentation-source tests, including eight
intake cases. They cover bounded metadata-only import, shared identity reuse,
unadmitted identity collisions, record size limits, raw decimal measurement
reports, evidence-link drift, unchanged retained-master content and reusable
source/occurrence scopes. Locked all-target/all-feature Clippy passes with
warnings denied. Actual project metadata intake succeeded separately; decoded
project parity and runtime adoption remain separate requirements.

Producer-applied provenance and editing checklists found two requirements:
measurement reports must remain exact asset bytes, and a reusable opening must
retain its original source identity independently of its episode occurrence.
The reference validator and explicit `presentation_scope` implement those
requirements. These checks are not independent review or creative approval.

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

The native source/context extension passes seven new scenarios, including an
actual guarded render, primary comparison, source-clock refusal, output-role
refusal, sparse cel changes, offline replay and recipe invalidation. Final local
regressions pass 215 library tests (nine explicitly ignored), one runtime-provider
test, five still-sequence tests and 78 Tig input tests (12 explicitly ignored).
Locked all-target/all-feature Clippy passes with warnings denied. Hosted checks
must qualify the exact proposed head before merge or runtime-pin updates.

The managed-verification extension passes 216 library tests (nine explicitly
ignored), all five still-sequence cases and all eight presentation-source cases.
Its ownership scenario runs with Git and working records/definitions hidden,
reopens frozen metadata, permits an independent peer, retains all five media
guards and rejects withdrawn ownership. Locked all-target/all-feature Clippy
passes with warnings denied. These remain synthetic results; actual project
acceptance and exact-head hosted qualification are separate.
