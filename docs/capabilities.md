# REEL capabilities

REEL connects creative authoring to reproducible media production. Its scope
includes story and series structure, picture and animation, voice and sound,
music, editing, editions and the engineering that makes revisions recoverable.
This guide groups the available contracts and tools by production purpose.

Start with [getting started](getting-started.md) for a synthetic planning example,
or [use cases](use-cases.md) for a complete workflow choice. Each linked guide
defines its supported inputs, feature flags, platform and external dependencies.
The [development status](development-status.md) distinguishes main from active
workflow development. A contract or successful technical check does not select a
creative master or authorize release.

## Story and series

REEL preserves authored intent from brief and format through scene, performance
and programme assembly. Ordered scenes, openings, poems, credits and reusable
programme parts have explicit controls rather than being inferred from folders.

- [Formats](../formats/), [styles](../styles/) and the
  [Rhythm, Emotion, Execution, Legibility rubric](../scoring/) guide creative work.
- [Production manifests](production-manifest-v0.2.md) and
  [scene authoring](scene-authoring-v1.md) describe scenes, semantic events,
  pictures, cues and language bindings.
- [Series control](showrunner-control-v0.2.23.md),
  [runtime and pacing audits](series-runtime-planning-v0.2.22.md) and
  [cue continuity](cue-import-and-continuity-v0.1.md) support episodic planning.
- [Programme parts](programme-parts-v2.md),
  [episode planning](tig-episode-planning.md) and
  [episode jobs](tig-episode-jobs.md) compose reusable material into longer works.

## Picture and animation

The visual toolset covers still-cel storytelling and more articulated animation:
picture selection and holds, exposure timing, layered performers, choreography,
camera motion, effects and mixed-media composition.

- [Cel sorter](cel-sorter-v1.md), [source inspection](cel-source-inspection-v1.md)
  and [insertion](cel-insertion-v1.md) provide scoped, source-locked edits.
- [Exposure sheets](animation-exposure-sheets-v0.2.43.md),
  [limited animation](limited-animation-v0.2.23.md),
  [sprite animation](sprite-animation-v0.2.23.md) and
  [layered sprite libraries](layered-sprite-libraries-v0.2.31.md) describe acting
  and timing beyond a sequence of held pictures.
- [Choreography](choreography-v0.2.24.md),
  [contact and holds](sprite-contact-and-holds-v0.2.28.md),
  [visibility windows](performer-visibility-v0.2.30.md) and
  [sprite emissions](sprite-emissions-v0.2.29.md) expose explicit staging controls.
- [Camera tracks](still-plate-camera-tracks-v0.2.45.md),
  [mixed-media timelines](mixed-media-timeline-v0.2.20.md),
  [effect passes](effect-pass-contract-v0.3.19.md) and
  [Motioncraft](motioncraft-authoring.md) connect picture and motion direction.
- [Browser compositions](deterministic-browser-compositions-v0.3.22.md) render
  supported HTML/SVG/CSS/JS sources on an explicit frame clock, with a
  [managed native worker](tig-browser-native-runtime.md).

## Voice and sound

REEL treats sound as part of scene structure. Spoken cues, alignment, music,
silence and effects feed the production clock and mix, with explicit provider
and performance boundaries.

- [Speech generation](tig-speech-generation.md) connects private, pinned
  attempts to the native production route.
- [Voice-performance direction](voice-performance-v0.2.15.md),
  [prosody evidence](voice-prosody-evidence-v0.2.16.md) and
  [cross-scene consistency](voice-consistency-v0.2.17.md) preserve direction and
  make performance checks inspectable.
- [Audio-event clocks](audio-event-clocks-v0.1.md) and
  [sample-clock controls](tig-sample-clock.md) connect measured audio to picture.
- [Dialogue/score mixing and stems](dialogue-score-mixing-v0.3.12.md),
  [speech-keyed dynamic EQ](speech-keyed-dynamic-eq-v0.3.13.md) and
  [sonic assets](sonic-asset-library-v0.3.18.md) support sound production.
- [Audio-quality checks](audio-quality-v0.2.11.md) and
  [exact stem recovery](tig-scene-audio-recovery.md) support verification and
  recovery. Provider access and voice consent remain project responsibilities.

## Music production

The music workflows span generation and auditions, inspection and repair,
editable notation and lyric models, arrangement, score export and mastering.
Recordings, canonical lyrics, analysis and creative selection remain distinct.

- [Song-generation contracts](song-generation-v0.2.25.md) and
  [local auditions](local-song-auditions.md) integrate explicitly installed
  generation/analysis runtimes, including the documented ACE adapter.
- [Song editions](NATIVE-SONG-EDITION.md),
  [masked repair](original-song-masked-repair.md) and
  [repair candidates](music-repair-candidate-v0.3.7.md) preserve original material
  and expose the scope of a proposed change.
- [Music interchange](music-interchange-intake-v0.3.2.md),
  [editable model drafting](music-model-draft-v0.3.5.md),
  [note editing](music-note-editor-v1.md),
  [lyric underlay](music-underlay-editor-v1.md) and
  [sung alignment](sung-lyric-alignment-v1.md) connect performances to editable
  musical representations and timing.
- [Arrangement plans](music-arrangement-plan-v0.3.10.md),
  [score export](music-score-export-v0.3.1.md),
  [instrumental scoring](music-instrumental-score.md) and
  [language adaptation](music-language-adaptation-v0.3.8.md) have separate
  source, runtime and review requirements.
- [Album mastering](NATIVE-ALBUM-MASTER.md) and
  [album video](NATIVE-ALBUM-VIDEO.md) assemble pinned recordings with deliberate
  gaps, chapters and checked sample transformations.

Optional generation, recognition and engraving adapters use external tools;
some existing adapters use Python. REEL does not bundle model weights or turn
an inferred score into an approved transcription.

## Editions and delivery

One authored work can have deliberate language, presentation and output
variants, with exact relationships to its source and delivery history.

- [Shared picture/language bindings](use-cases.md#share-pictures-across-languages)
  retain common visual intent with language-local recordings and timing.
- [Episode captions](episode-captions-v1.md),
  [speaker-aware captions](speaker-caption-presentation-v0.2.9.md),
  [accessibility checks](caption-accessibility-v0.2.8.md) and
  [caption-safe layout](caption-safe-picture-layout-v0.2.48.md) address reading
  and viewing requirements.
- [Native print collections](native-print-collections.md) produce supported
  manuscript, songbook and bilingual PDF packets from pinned text, fonts,
  score SVG and retained PDF pages.
- [Presentation overlays](film-presentation-overlay-v1.md) and
  [OpenTimelineIO export](otio-export-v0.2.40.md) provide additional editing and
  interchange routes.
- [Delivery history](use-cases.md#track-a-delivery-over-time) and
  [saved delivery observations](tig-delivery-observation.md) record exact
  target/revision relationships. Upload and remote verification require the
  producing project's publisher and evidence.

## Production engineering

The engineering layer makes it possible to change one part of a production,
understand what it affects and retain the source and evidence for earlier work.

- [Changed-only planning](changed-only-build-v0.2.41.md) and
  [native input bindings](tig-native-scene.md) identify declared dependencies,
  exact source views and revision-sensitive work.
- [Managed jobs](tig-job-runner.md),
  [frozen requests](tig-frozen-job.md) and
  [checkpoint recovery](jobs-checkpoint-resume.md) retain explicit input/runtime
  identities, budgets and recovery boundaries.
- [Decoded media comparison](native-media-parity.md),
  [retained delivery checking](tig-retained-delivery-check.md),
  [delivery inspection](tig-delivery-inspection.md) and
  [episode control inspection](tig-episode-control-inspection.md) connect outputs
  to their source and inspectable frame/sample evidence.
- [Tig integration](tig-public-integration.md) supplies exact asset resolution
  and retention/history adapters. [Accounting](asset-manager-workflow-v1.md)
  and [version comparison](asset-version-diff-v1.md) support explicit storage
  review; generic asset identity and lifecycle services belong to Tig.
- The [WASM workbench](https://giodl73-repo.github.io/REEL/assembly/) inspects
  semantic graphs and locks in a browser. It is distinct from media rendering
  and the [cel/note/underlay editor host](editor-host-v1.md).

Managed native protection currently has Windows-specific requirements. Other
routes have their own platform/runtime limits. Technical checks, creative
approval, rights and storage-removal authority remain separate decisions.

See the [architecture](architecture.md) for component ownership, the
[reference index](reference-index.md) for individual contracts and the
[development status](development-status.md) for work proceeding separately.
