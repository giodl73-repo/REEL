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
handoff through existing systems. Complete derived recipe/managed enrollment,
CLI adoption and actual Recuerdos authoring remain open. A new cel updates its
leaf; subsequent lowering changes the consuming Job pin without editing its
parent. Older archived sources continue to lower their original pins.
