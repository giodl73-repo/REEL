# Tig delivery templates

Experimental preparation API: tig_delivery_template::lower(Source, Request).
Entry b1ce0b5fa8c27f0374a200d870168217761f3a1c. Native producer scope; Tig
continues to own exact source/byte identity. Existing scene-delivery, audio-event
clock, Tig input and job contracts remain authoritative for their own phases.

A selected source record authors binding_ids (alias to exactly {id}) and
control_ids with exactly job, contract, production. The request admits distinct
root, primary and control definitions through the existing exact source profile.
Controls are independently versioned and selected. authored_control_fields may
explicitly map a control definition to an authored object field such as document.
Other control definitions use their exact admitted selected/named-stream selector
through the existing native control adapter. Large controls stay asset-side.
Authored controls replay directly from the metadata archive; selected controls
still need their pinned byte objects. Physical placement remains provider-owned.

The job document uses schema reel.tig-delivery-template.experimental-1 and the
existing delivery fields. Each input FileRef position instead contains exactly
{"binding":"alias"}. Contract and production_manifest_sha256 are generated;
authored competing values refuse. The separate contract names production.json
as its virtual production document. Existing compiler rules own clock namespaces,
integer samples, frames and attachment semantics. No speech cues are synthesized.
Lowering replaces deferred inputs with current ID/hash/length FileRefs and returns
the normal typed Job plus exact contract/production bytes and consumed bindings.
It creates no directory and never mutates source records.

Every catalog reference must resolve to an admitted primary type. Used inputs
and controls must carry an explicit selected state. Unused draft alternatives
need not have bytes; used drafts refuse. Variant maps refuse pending an explicit
variant admission rather than silently selecting a language. Timed overlay
receipts additionally verify all their exact nested input references against
the same primary catalog; stale or unlisted pins refuse. Only explicitly selected control/receipt
metadata is read under the existing bounded guarded provider. Media is not read.

Bounds: 4,096 binding aliases, 256 primary and 64 control types; three controls of
at most 4 MiB each / 16 MiB aggregate, 4 MiB lowered job, 4 MiB per derived receipt / 16 MiB aggregate,
4,096 entries per delivery collection/receipt. No discovery, global lock,
provider initialization, hydration or transfer. Input reads retain the caller's
existing byte-source request/time limits. Alias lookup is indexed in memory.

Preparation is not a valid render receipt, output freshness decision or custody
claim. The returned checkpoint identifies metadata; it does not adopt a job or
hold payloads. Callers still need exact producer/recipe/runtime/resource/source
admission, guarded prepare, native plan/render/check, result retention and owner
handoff through existing systems. Complete managed enrollment,
CLI adoption and actual Recuerdos authoring remain open. A new cel updates its
leaf; subsequent lowering changes the consuming Job pin without editing its
parent. Older archived sources continue to lower their original pins.

## Scoped requirements and recipe contexts

`requirements` resolves the controls and consumed primary roots through the
existing Tig graph. Unused alternatives are not dependency roots. This is an
exact local production scope, not global demand or permission to remove bytes.
`freeze_context` returns prepared controls and an ordinary Tig ProductionContext
candidate through caller-supplied metadata ports. The descriptor pins the request,
adapter code and lowered job/contract/production objects. Source provenance stays
separate from consumed-input recipe identity; no per-project cache engine or
another authored asset inventory is introduced.

`restore_context` checks the admitted archive, exact roots, requirements and
freshly lowered descriptor before binding the restored source. It uses the old
source explicitly, never a newer checkout. Selected control objects still need
their declared existing byte provider. `recipe` checks the frozen source/scope
and derives a projected Tig recipe with an explicitly admitted producer object.
An unused-alternative or annotation edit can preserve a fingerprint while having
different source provenance; changing a used leaf, control or producer changes it.
Lowered FileRefs bind actual consumed IDs, so omitting the whole alias catalog
from the consumed-field projection does not erase effective input changes.

None of these APIs attests to actual producer consumption, installed runtime,
byte availability or custody. Context finish/readback, owner adoption, resource
admission and worker/result handoff remain separate. Recipe equality alone
cannot skip eligibility checks, authorize output reuse or claim a current render.

## Checked candidate execution

`tig_scene_production::render_delivery_with_programs` checks the frozen recipe,
prepares guarded exact primary inputs and borrows declared runtime capabilities.
The lowered controls stay in memory through the existing InputFiles control port.
`scene_delivery::render_with_control_files` uses the same rendering core as the
file-based entry point and independently checks the staged result before normal
publication. It does not create another loose job/contract/production directory.

The returned existing RenderedCandidate supports the existing exact output-subject
claim and candidate publication APIs. Rendering, receipt verification and output
mapping still precede a claim; foreign output subjects and existing destinations
refuse. Publication acknowledgments remain distinct from independent custody or
owner handoff. This native producer function does not reserve space or enroll a
job: the caller must already admit/own source, runtime, scratch and output capacity
through the existing runner. Managed worker/CLI integration remains required.

`jobs::JobPlan::from_delivery_context` now admits the delivery recipe into the
same runtime manifest, freeze/readback, owner-protection, render/publication and
output-handoff protocol used by native scenes. Its private request variant keeps
delivery interpretation explicit. Scene-only parent/reference APIs return an
error for delivery jobs; no fabricated scene or speech request is constructed.
The shared render method leaves a delivery's proposed preparation directory
absent, since the controls stay in memory. Existing owner, revision, journal and
post-handoff checks still run before and after producer execution.

This provides the managed SDK ownership path. CLI/process containment and actual
project capacity/source enrollment still need their existing runner integration;
constructing a plan does not reserve disk space or establish independent custody.

## Explicit CLI workflow

The existing `reel-tig-job` commands also accept delivery requests:

```text
reel-tig-job capture-context delivery-capture.json private-fragment.json
reel-tig-job plan-outputs delivery-job.json
reel-tig-job run delivery-job.json
```

Capture uses `reel.tig-delivery-capture-profile.experimental-1`, the committed
source profile, a delivery-template request, existing metadata/cache bindings
and a trusted metadata minimum. Omit `preparation_cache`: no loose controls are
generated. Capture reserves metadata growth, finishes/reopens the provider and
returns a private fragment with `context_mode: "delivery"`, exact archived source,
request and context. It does not initialize stores, adopt a job or render media.

Use that fragment in the existing `reel.tig-native-job-profile.experimental-1`
worker. Add explicit runtime pins, source/journal/owner/revision bindings, active
cache and capacity settings, workspace/scratch/time budgets and either exact
output caps or an output-budget policy. The source request's schema must match
the delivery mode; scene/compiled modes cannot silently interpret it. Small and
large selected control objects retain their existing provider admission rules.

Planning derives the native output set directly from in-memory controls, then
applies output caps. The render worker checks that set before encoding and checks
actual output sizes afterward. The existing contained child runner reserves
resources, holds source/runtime obligations, retains outputs and retires owned
scratch. Optional record-output templates and Git handoff use the same existing
bounded candidate proposal path. Publication/retention still grants no human
approval, current external delivery or independent backup custody.
