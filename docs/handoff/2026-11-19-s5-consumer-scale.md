# S5 consumer scale and workflow qualification gap

The S4 assembly lane reported an E7 parent with approximately 600 resources and
7.94 GB, with individual masters around 2.9–3.36 GB. These are consumer-reported
requirements, not a local media inventory or a completed REEL qualification.
No S4/S5 job, selected take, profile, media cache or approval was changed here.

The retained consumer contract was subsequently read and matched to its selected
record at exact Melena planning commit
`f01f06fbab40b7566ca7c578f2036dd73f9b48ee`:

- Asset ID: `planning/season-05-opening/workflow-requirements/v1`.
- Record: `episodes/season-05/opening/preparation/workflow-requirements-v1.tig`.
- Payload SHA256: `2a665407e45639f6f3b0bfe7426f0d52821580f20b1b02827148540595ea07d1`;
  5,135 bytes. Its inspected REEL checkpoint is `e35a9b6`.
- Schema: `melena.reel-workflow-consumer-requirements.v1`; production readiness
  and public release are false. Existing cached bytes were read without asset
  restoration, hydration, publication or production adoption.

The exact reported logical total is **7,936,104,893 bytes**, and the consumer
requires five-hour render windows. Source captions comprise fourteen tracks /
787 cues and 336 Spanish review-label spans; these counts do not establish
whole-film conformance. Melena retains the contract payload and creative choices;
REEL stores this small requirement/evidence mapping.

The user's subsequent planning context names the retained working theme
**Olas perdidas** at the full 2:00, a proposed Cuba / 1958 first card and a
departure/Miami direction. These remain project planning choices, separate from
technical acceptance and production adoption. The existing S4E7 jobs and exact
approvals continue unchanged.

The bounded movie/comic fixtures in the completion ledger do not establish this
scale. Keep actual input count, logical byte total, largest object, scratch,
output size and lifecycle transaction changes distinct during admission.

| Current boundary | Implementation | Implication for the reported consumer |
| --- | --- | --- |
| 128 task inputs/source bindings; 128 plan tasks | `reel-workflow/src/lib.rs` | A flat 600-input task cannot be admitted. |
| Comic input closure 64 MiB, output 128 MiB | `reel-workflow/src/publication.rs` | Comic-specific limits; do not describe 64 MiB as a universal film input limit. |
| Film output 128 MiB and workspace 512 MiB | `reel-workflow/src/film_render.rs` | Multi-GB masters cannot use the present movie task contract. |
| Film process timeout 3,600 seconds and aggregate raw RGB 16 GiB | `reel-workflow/src/film_render.rs` | Five-hour render windows and full-episode frame demand need an explicit bounded composition contract. Runtime budget and film duration remain distinct. |
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

## Retained consumer requirement mapping

| ID | Acceptance requirement | Current boundary / remaining evidence |
| --- | --- | --- |
| S5WF-01 | Streamed large-resource composition and explicit bounded scene/chapter decomposition on existing F-first storage | Limits above refuse the reported production. Generic protocol and native lifecycle qualification remain required. |
| S5WF-02 | Owned project profile adoption/rollback; reuse corrected S5 recordings and current/prior media | Historical input/retention fixtures exist; real Melena adoption, rollback and selected-take reuse are open. |
| S5WF-03 | Stable story/C IDs across immutable drawing versions; exact scene/profile approvals; affected consumers only | Review-ID fixtures exist; versioned-story identity, drawing substitutions and affected-language/episode invalidation require production integration. |
| S5WF-04 | Complete bilingual parent with chapters, poem/title/attribution breaths, lyric-synchronized opening, full poem-song closing over episode cels and actual-use credits | Components and the architecture target exist; complete consumer assembly and semantic/actual-use checks are open. |
| S5WF-05 | SRT/VTT from retained conform receipts and selected-take clocks; differing ES/EN timing; complete decode/frame/sample/order and scoped image/listening checks | Language-local trigger controls are qualified in bounded fixtures; full edition clocks, caption conformance and actual modalities remain open. |
| S5WF-06 | H264/MP3 local review; H264/AAC 48 kHz stereo ES/EN and ES-ID; exact ID tables and eleven-file private handoff; shelf fallback and prior deliveries | Complete workflow delivery integration and target checks remain open. Private, public and Golden authority are separate. |
| S5WF-07 | Interrupted long-movie/controller recovery, completed-render adoption, original runtime/work/claims and no duplicate effects | Bounded movie/history/replay evidence exists; interruption/adoption at consumer scale remains open. Routine technical steps must not create repeated creative approval prompts. |
| S5WF-08 | Declarative cast role/age, exact likeness sheet, wardrobe/jewelry, selected voice consent/stage, location/date and pacing; distinct narrator/poet roles | Direction abstractions exist; exact project data bindings and actual creative/rights decisions remain with Melena. Private identity details must stay out of shareable review evidence. |

### Selective rebuilds across Git revisions

`make_packet` currently copies the plan-wide `source_commit` into every packet;
`packet_key` includes that commit. Thus advancing the overall plan's commit also
changes otherwise identical task identities. Existing within-snapshot isolation
tests do not establish S5WF-03 across a real Git revision.

Project adoption needs explicit task/producer snapshot and retained-result
adoption semantics, so unchanged consumers can stay bound to their original
exact snapshots while affected consumers advance. Graph, repair, review and
native bindings must agree on that boundary. Qualify a real A -> B revision in
which an affected-language task changes, the unaffected task/result/native
binding stays identical, and only its actual downstream dependents rebuild.
Keep exact commit/record/annex-leaf identities in all keys and checks; dropping
the commit pin or substituting current/latest is not selective invalidation.
