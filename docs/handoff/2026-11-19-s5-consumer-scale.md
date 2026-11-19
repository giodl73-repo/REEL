# S5 consumer scale and workflow qualification gap

The S4 assembly lane reported an E7 parent with approximately 600 resources and
7.94 GB, with individual masters around 2.9–3.36 GB. These are consumer-reported
requirements, not a local media inventory or a completed REEL qualification.
No S4/S5 job, selected take, profile, media cache or approval was changed here.

The bounded movie/comic fixtures in the completion ledger do not establish this
scale. Keep actual input count, logical byte total, largest object, scratch,
output size and lifecycle transaction changes distinct during admission.

| Current boundary | Implementation | Implication for the reported consumer |
| --- | --- | --- |
| 128 task inputs/source bindings; 128 plan tasks | `reel-workflow/src/lib.rs` | A flat 600-input task cannot be admitted. |
| Comic input closure 64 MiB, output 128 MiB | `reel-workflow/src/publication.rs` | Comic-specific limits; do not describe 64 MiB as a universal film input limit. |
| Film output 128 MiB and workspace 512 MiB | `reel-workflow/src/film_render.rs` | Multi-GB masters cannot use the present movie task contract. |
| Contained-process input hash bound 512 MiB per file | `reel-workflow-native/src/film_process.rs` | Streaming hash implementation still explicitly refuses the reported individual masters. |
| Owned input staging 512 MiB maximum reservation | `reel-workflow-native/src/staging.rs` | Materializing the reported full closure is unsupported. |
| Generic candidate return total 128 MiB | `reel-workflow-native/src/response.rs` | Current return ingestion cannot admit multi-GB masters. |
| Resolution resource closure 126 objects / 512 MiB | `reel-workflow-native/src/resolution_job.rs` | The qualified one-page repair resolution does not cover an E7-sized closure. |
| Atomic admission change budget; 16 total actors | `reel-workflow-native/src/admission.rs`, qualified Tig SDK | Media/task counts alone do not predict feasibility; previews include source/record/runtime metadata. No implicit per-chunk actor allocation. |

## Required implementation and evidence

1. Define a streamed producer/return contract over guarded exact native objects,
   with explicit total/per-object/output/scratch/time/process bounds. Inputs and
   output hashing must use bounded buffers; no whole-master `Vec` requirement.
   Reference existing F-first media/takes through an owned pinned consumer
   profile. Historical reads must not depend on a CAMINOS/CAIMITOS checkout.
2. Define explicit bounded composition and retained intermediate boundaries for
   the real closure. A small manifest or packet must not hide unprotected media.
   Preserve all original record/annex-leaf/stream bindings and the complete
   declared closure. Do not raise constants, silently split admission, omit
   resources, substitute latest bytes or create a new cache engine.
3. Qualify the required managed admission/protection transitions with Tig. The
   currently qualified atomic admission does not establish a resumable 600-item
   protection protocol. Each lifecycle phase must obey the 128-change bound,
   the whole run must obey the actor-table limit, and a final retained owner must
   cover every required store before a source slot can close. Any new protocol
   needs exact source/tool pins and crash fixtures; it is not inferred from byte
   streaming or content deduplication.
4. Preserve stable story/cel identities across drawing versions and explicit
   affected-language/consumer rebuilds. Reuse corrected S5 takes. Source words,
   approval subjects, voice/likeness restrictions and source rights remain
   producer-owned; technical receipts cannot approve changes or release.
5. Qualify complete bilingual assembly: chapter poem breaths, lyric-triggered
   opening, full poem-song ending over episode cels, actual-use credits, captions
   from the selected receipt/sample clock, and declared H264/MP3 local plus AAC
   portable/ES-ID editions. Existing language-local trigger controls and review
   IDs are components, not proof of the complete assembly or actual translation.
6. Interrupt the real-size synthetic/native producer at preparation, rendering,
   publication, retained acknowledgement and completion. Reconcile original
   identities without duplicate effects, retain the exact result and prove owned
   profile rollback/history after slot reuse. Missing bytes must refuse; timeout
   or cancellation must preserve claims and scratch pending explicit recovery.

The current automatic resolution qualification addresses a separate missing
dispatch boundary using the existing small retained comic lineage. It must not
be represented as solving these film scale, content or delivery requirements.
