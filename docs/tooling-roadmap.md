# Reusable authoring tools roadmap

Checkpoint: 2026-10-23. REEL owns generic Rust engines, explicit CLI/UI entry
points and synthetic fixtures. Project repositories own private declarative
controls, media, exact versions, authority and delivery choices. Merging source
does not install a binary or retarget an active project/runtime.

| Area | Available source | Remaining work |
| --- | --- | --- |
| Asset/cache foundation | Local LFS, version/dependency accounting, tier planning/review, guarded verified removal, typed references and admitted jobs. Recent main work includes checkpoint resume and registered bounded execution windows (PRs 145, 146, 150). | Adopt exact tested runtime/profile pins per producer. Keep complete demand/custody coverage and explicit removal decisions. An old inventory or CI pass alone grants no deletion authority. |
| Cel discovery/editor | Shared native reader, Rust load/apply API, static editor export, explicit file maps, effective pictures, notes, version locks, append-only picture revisions and cue-local timing (PRs 147/148). One scoped language in a full multilingual scene, up to 128 total cues; combined or explicitly separate language graphs. Metadata-only inspection emits a locked edit template. Legacy drafts stay compatible. | Browser visual acceptance, private media/render/review and older-version reopen. A real frozen bilingual episode now passes identity/reorder/removal and both-language native delivery-control compilation. Explicit runtime adoption follows the remaining proof. Simultaneous linked language editing, inserting new assets and wider media formats remain later extensions. |
| Scene/project adapter | Existing native scene, scoped bindings, measured alignments and delivery compiler. The cel editor routes existing layouts without another scene grammar. | Replace remaining legacy source-repository/Git-blob assumptions with explicit project inputs. Prove one episode and one music-video family without archived builders. |
| Scene execution | Existing admitted jobs, output retention and explicit resume; native rendering/composition already exist. | Compare the preserved executor against REEL capabilities and port only missing generic operations. Keep source/executable pins and exact output/reference accounting. |
| Songs, scores and arrangements | Native song generation, music source/analysis/model contracts, MIDI/MusicXML/SVG export, arrangement and printable-score engines. Bounded note and exact-text lyric-underlay workshops create new unreviewed native model proposals with source locks. Selected lyric bytes and authorities remain unchanged. | Browser and private-song acceptance; phrase/arrangement decisions and finished engraving. Explicit musical/lyric review remains essential. Project adoption of an unreviewed melody draft does not establish finished notation. |
| Large inventory/graph infrastructure | Separate draft PR144 has bounded index/adapters, revisioned catalog and fenced publication fixtures. | Cache-wide/all-consumer enforcement, cross-domain routing/lock order, safe explicit activation and final canonical unlink remain integration work. Green fixture CI does not close these gaps. Keep separate from routine cel UI adoption. |
| Speech and episode assembly extensions | Separate draft PR120 (local speech/alignment) and PR128 (bounded lossless assembly). | Their owners must close the documented adoption/review gaps. They are not implicitly merged by the cel-tooling milestone. |

## Next bounded units

1. Close cel-tool adoption with one explicitly chosen private multi-cue scene:
   open the supplied project, edit within cues, preserve comments, apply to a
   new candidate, verify exact inputs, compile/render through the existing job
   system, review, and reopen the previous locked version. Browser layout and
   media acceptance are distinct from JSON/compiler tests. No automatic server.
2. Complete explicit scene/project adapter coverage for that episode family and
   a music-video family; reuse the existing native contracts and execution.
3. Prove the [note workshop](music-note-editor-v1.md) and
   [exact-text underlay](music-underlay-editor-v1.md) on a private native model
   through validation and score export, with browser and musical review kept
   separate. Extend explicit phrase/arrangement authoring over the
   existing Rust contracts. Keep source text, recordings and score editions separate.
4. Continue managed-index/fencing adoption as its own infrastructure task with
   complete consumer coverage and explicit operational policy. It must not
   silently activate cache removal or expand the current editing milestone.

## Evidence and boundaries

Cel source/compiler evidence is in [the editor contract](cel-sorter-v1.md),
[project paths](cel-sorter-project-v1.md) and the
[compiler round-trip review](reviews/2026-10-23-cel-compiler-roundtrip.md).
Public fixtures cover native candidate application and actual delivery-control
compilation; prior private checks cover metadata-only 40/40/39-event round trips.
The [separate-graph review](reviews/2026-10-23-cel-episode-graph-files.md)
records twelve actual compiler checks over a frozen private bilingual episode's
six candidates. Those results do not certify media, rendering or browser layout.

Further contracts: [typed references](asset-references-v1.md),
[tier review](asset-tier-review-v1.md), [jobs](asset-jobs-v1.md),
[checkpoint resume](jobs-checkpoint-resume.md),
[scene authoring](scene-authoring-v1.md),
[music export](music-score-export-v0.3.1.md),
[arrangement plans](music-arrangement-plan-v0.3.10.md) and
[printable lead sheets](music-printable-lead-sheet-v0.3.16.md).
Private project port maps should link these contracts and exact source commits;
large inventories/reports remain assets rather than copied roadmap prose.
