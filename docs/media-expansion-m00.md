# M00: spoken-edition reuse map and implementation boundary

**Baseline:** `d923375398beb83a5c7eb27896ef15838399f135` (main after PR #377).
**Scope:** the first mapping/brief step of the [16-PR plan](media-expansion-pr-plan.md).
This record specifies work; it produces no speech or media and changes no asset
definitions, runtime bindings or vendored SDK bytes.

The [fixture and test specification](media-expansion-spoken-fixture.md) pins the
scripts, delivery profiles, sample arithmetic, commands and core test inventory.
It supplies the concrete choices required by the [test matrix](media-expansion-test-matrix.md).

## Reuse, extension and new behavior

Paths/symbols below were inspected at the baseline. An existing primitive is not
evidence that the complete proposed edition path already exists.

| Requirement | Existing owner / exact surface | Supported boundary | M01 onward decision |
| --- | --- | --- | --- |
| Source/language/cue identity | [scene authoring](../crates/reel-assembly/src/scene_authoring.rs): `SCENE_SCHEMA_V2`, `Cue`, `Language`, `CueTakeSegment` | Exact text hashes, speaker IDs, take/alignment bindings, several speaker-tagged segments and language-local event bindings already exist | Reuse these meanings and IDs. Add an audio-edition sidecar selecting module/cue versions; do not replace scene v1/v2 or synthesize pictures |
| Shared author intent versus measured phrase timing | Same module: `SharedEvent`, `LanguageEventBinding`, `compile_selected_event_request`; [cue-relative compiler](../src/cue_relative.rs): `Contract`, `CueClock`, `CueMarker`, `compile_with_manifest_bytes` | Integer sample markers and distinct narration/audio-event namespaces; cue clocks concatenate native durations; targets bind existing production objects | Reuse clocks/marker semantics and checked arithmetic. Edition-specific gaps/overlaps/order need their own compiled placement representation; do not relabel an arbitrary recording a narration cue |
| Reusable semantic module and context | `Cue.source_id`, `source_cue_ids`, exact text and selected asset identity provide source linkage | No inspected contract declares audio module dependencies, executive/technical selections or per-edition context closure | New narrow sidecar: stable module identity, ordered speaker cues, required module/context IDs and exact selected variants. Validation checks declarations; Story/source authority checks truth |
| Spoken scripts, direction and observations | [voice performance](../src/voice_performance.rs): `write_plan`, `check`, `write_prosody_evidence`; [speech generation](../src/speech_generation.rs) | Direction plans distinguish requested/executed/advisory dimensions. The private adapter is explicit; its request text does not establish performed words | Reuse exact text/performance/evidence distinction and existing adapter for any later authorized recording. No new TTS engine or implicit provider call |
| Natural handoffs/protected material | `CueTakeSegment` retains speaker/sample identity; [scene audio](../src/scene_delivery.rs): `Audio`, `AudioPlacementOffset`, `audio_placement_samples` | Explicit source offset, gain/fades and signed placement already exist, within a complete scene plan | Add edition-level adjacency policy, recorded/added silence distinction, protected word/breath/tail intervals and authorized overlap. New contexts require fresh review |
| Exact input access | [scene delivery](../src/scene_delivery.rs): `FileRef`, `InputFiles`, `MediaProgram`; native SDK byte readers | Existing ports hold/check exact selected bytes and explicit media tools | Reuse `FileRef` and borrowed input/tool-provider patterns; no path discovery or new byte store. Keep local staging distinct from asset admission |
| Audio-only production | `scene_delivery::render_audio_with_inputs`, `check_audio_with_inputs`, `AudioReceipt` | Produces/checks audio stems, but first runs the full scene planner; `plan_from_bytes` requires picture coverage. Audio checker checks receipt/PCM format and counts, not an independent arbitrary programme waveform oracle | Extend reusable PCM/port helpers where appropriate; add a bounded audio-edition producer/checker that accepts no picture input. Do not claim the existing stem path already satisfies this requirement |
| Mix/stems | [audio mix](../src/audio_mix.rs): `compile`; `scene_delivery::render_audio_stems` | Existing D/M/E policies and filters, with some timing expressed in milliseconds/seconds | M03 starts with exact lossless dialogue-only sample assembly. M07 adapts existing mixing only with explicit sample-placement and decoded-result proof; do not silently round semantic boundaries to milliseconds |
| Composition of retained components | [prefix composition](../src/episode_conform/parts/composition.rs): `Composition`, `compose` | Exactly two complete video source sets, checked manifests/producers and video clocks; no subsets/trims/crossfades | Retain untouched. Its source/proof separation is useful precedent, not the implementation of arbitrary speech-module assembly |
| Transcript and chapters | [episode captions](../src/episode_captions.rs): `Manifest`, `assemble`; [caption presentation](../src/caption_presentation.rs): `CaptionPresentationInput`, `DeliveryCueAssignment` | Selected caption tracks shift on checked episode clocks; explicit cue-to-speaker-role mapping; episode/video dependencies remain | Reuse text timing/role semantics. M04 adds audio-edition transcript/chapter output and independent correspondence checks; no fake episode receipt |
| Consumed recipe identity | [pinned SDK recipe](../vendor/tig/crates/tig-assets/src/graph/recipe.rs): `Consumption`, `PayloadConsumption`, `plan_with_payloads` | Exact declared consumption/recipe identity with existing graph/runtime admission | M05 projects each edition's actual consumption into admitted interfaces. Unused language/detail variants must not affect its recipe. Generic missing behavior goes upstream |
| Publication/retention | [publisher](../src/tig_publication.rs): `ExistingPublisher`, `PublicationBackend`; [episode planner](../src/tig_episode.rs); [SDK publication](../vendor/tig/crates/tig-assets/src/publication.rs) | Existing explicit stores, bounded publication and retained source/context evidence. Source pin is `0ceade2a16de1c1ed6cc27e48675e7945d7b61df` in [source pin](../vendor/tig/source-pin.json) | M05 qualifies this exact interface for spoken candidates; no cache initialization, automatic restore, generic storage fork or custody claim |
| Retained audio recovery | [scene audio recovery](../src/tig_scene_audio_recovery.rs): `run` | Exact retained scene job/receipt recovery, not new programme authorship | Keep separate; do not use recovery as proof that a newly authored edition is current |

## Proposed code ownership and command surface

M01 defines `reel.spoken-edition.v1` as a **proposed** additive contract. Existing
scene/media/asset schemas stay unchanged. Put reusable edition structures and
validation in `src/spoken_programme.rs` (split into submodules as it grows), using
the existing crate's hashing, strict Serde and input-reference conventions.
Do not put producing-project manuscripts or private voice selections in REEL.

Reserve these CLI names; none exists at M00:

| PR | Proposed command | Allowed effects |
| --- | --- | --- |
| M01 | `reel spoken-edition-validate EDITION` | Read bounded local declaration; validation report only; no tool/provider execution |
| M02 | `reel spoken-edition-compile EDITION --output NEW_PLAN` | Resolve declared control evidence and write one new plan; no media execution |
| M03 | `reel spoken-edition-produce PLAN --asset-root ROOT --output NEW_DIR` | Explicit bounded audio execution from admitted/staged inputs; new candidate only |
| M04 | `reel spoken-edition-check EDITION PLAN --asset-root ROOT --candidate DIR` | Reopen sources/output, independently check media/text; no promotion or publication write |

M01 may implement validation and declaration-only fixtures; M02 adds placement
compilation; M03 adds execution; M04 adds independent verification/text packages.
Compiler/producer reports are not substituted for later checker evidence. M05
integrates through the existing asset/publication ports; it does not introduce a
generic registry CLI in REEL. CLI errors/reports obey existing private/portable
boundaries; no raw private text/path/authority values in normal output.

The active workflow lane has no advertised matching remote branch and is not on
main at this baseline. No new public workflow contract was found in this fetch.
It remains an interface dependency to recheck, not a source to edit or import.

## Review and M00 disposition

One assistant applied the six spoken-plan checklists: Story, Editor, Sound,
Platform/Audience, Vocal Adaptation and Rights. This is technical planning review,
not independent review or creative authority. Key tensions and dispositions:

- **Reuse versus truthful scope:** existing audio stems still require pictures;
  arbitrary programme assembly remains new bounded behavior. The map states both.
- **Naturalness versus sample proof:** synthetic timings below are exact test
  values, not measured speech. Listening remains a separate future decision.
- **Reusability versus context:** the executive closing deliberately has no
  reference to the omitted expert; required context and unsupported reorderings
  still have rejection cases. No take is universally interchangeable.
- **Privacy versus reproducibility:** public scripts are original fixture prose;
  no real casting/provider/consent identity is selected. Portable evidence uses
  controlled canaries and excludes local/private state.

M00's required source mapping, core brief values, oracle arithmetic and test
inventory are recorded. No blocking planning finding remains in this scope.
Exact runnable implementation tests are specified but do not exist yet.
Documentation checks and Cargo target inventory inspection do not qualify audio.
