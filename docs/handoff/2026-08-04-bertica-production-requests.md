---
work: bertica-cross-repo-request
stage: handoff
format: short-film
style: illustrated-2d
source_scenario: C:/src/bertica
author: bertica-production
rubric_version: v0.1
created: 2026-08-04
updated: 2026-08-04
sources:
  - C:/src/bertica/production/reel
  - C:/src/bertica/production/visual
  - C:/src/bertica/production/prevoice-scene-packets
---

# BERTICA requests for REEL

Please read this as a consumer-driven improvement request from *El camino de los
caimitos*. BERTICA will continue to own manuscript truth, voice consent, private
photographs, character facts, and approval. REEL should provide a portable
production grammar and deterministic transformations; BERTICA should not need a
runtime dependency on REEL internals.

## Current production evidence

BERTICA has built five private illustrated Spanish animatics through REEL-shaped
YAML manifests: Moro, Herrera arriving in Melena, Don Tancredo/the wedding, Papo
and Cachita, and the Nochebuena Riverita rescue. The experiments exposed four
repeated costs:

1. We must prepare scenes before an approved voice exists, but the current
   contract requires positive scene and shot timing.
2. When Bertica's narration was judged too fast, changing the effective pace to
   85 percent required shot starts/durations, captions, audio segments,
   transitions, and total export duration to be kept synchronized manually.
3. Andresito's poem and Bertica's prose are separate speakers with an important
   protected pause, but the manifest treats narration mainly as shot text and a
   single broad `narration_voice` direction.
4. Character likeness and continuity are increasingly photo-grounded, while the
   original private photographs must remain local and must not be silently sent
   to an image/video provider.

The next BERTICA queue begins with a Mayabeque beach scene: an overloaded launch,
a large ensemble with fixed seating geography, a rising waterline, a family
quarrel, a near-capsize, Amado Rosa pushing the boat back with his shoulder, and
a mother-child reunion. It is a useful stress test for all requests below.

## P0 — Untimed pre-voice manifests

Please add an explicit planning stage in which scenes and ordered shots may exist
without invented durations.

Desired behavior:

- A manifest can declare `timing_status: untimed|guide|conformed|locked`.
- Untimed shots preserve order, source cues, visual purpose, camera/motion intent,
  speaker, and narration/caption text while omitting start/duration.
- `validate` accepts untimed planning manifests but clearly reports that render,
  preview, caption export, and delivery commands are gated.
- `plan` or a dedicated command produces a useful untimed shot/storyboard plan.
- A later deterministic conform operation creates a new timed derivative rather
  than overwriting the planning source.

Acceptance test: validate and plan the Mayabeque sequence before any audio file
or duration exists, while render-oriented commands fail with a precise
`timing not conformed` message.

## P0 — Audio conform and atomic retiming

Please make voice-driven timing a first-class operation.

Desired behavior:

- Accept one or more measured narration cue files, each with speaker/cue identity
  and optional protected head/tail silence.
- Conform shots from cue durations and declared visual allocation rules.
- Support a deterministic global or per-speaker tempo derivative (for example,
  Bertica at 85 percent effective tempo while leaving Andresito and the
  poem-to-prose pause unchanged).
- Recompute as one atomic transformation: scene spans, shot starts/durations,
  narration segments, caption cues, transitions, platform/export duration, and
  review summaries.
- Refuse partial output if any dependent timeline becomes inconsistent.
- Preserve lineage: input manifest, audio hashes, transform parameters, output
  manifest, and tool version.

Acceptance test: take a two-speaker poem/prose scene, slow only Bertica, preserve
the exact Andresito timing and protected pause, and produce a manifest whose
scene, shots, captions, audio, and export totals validate without manual edits.

## P0 — Speaker-aware narration cues

Please separate narration identity from shot prose.

Suggested portable concepts:

- `speakers`: stable IDs, display names, language, pronunciation profile,
  performance direction, consent/approval status reference, and whether the
  asset is human, guide, or synthetic.
- `narration_cues`: stable cue ID, speaker ID, exact text or upstream text
  reference, source block range, shot/scene association, take/audio reference,
  and pause/breath policy.
- Protected transitions such as `poem_to_prose_pause` that conform/retime tools
  cannot change unless explicitly unlocked.

REEL should store approval references and gates, not infer that a voice is
authorized merely because an audio path or speaker name exists.

## P1 — Source-range and omission provenance

Memoir adaptation needs finer provenance than a single scenario path.

Please support:

- Contiguous and discontiguous upstream ranges such as manuscript blocks
  `1065–1073` plus `1115–1140`.
- Per-cue and per-shot source references.
- Explicit omissions and the permitted bridge: silence, title card, archival
  image, or separately approved adaptation.
- A generated source-coverage report that makes invented narration and
  unattributed dialogue easy to detect.

Acceptance test: the Mayabeque manifest discloses blocks 1074–1114 as omitted
and proves that every spoken line comes from one of the selected ranges.

## P1 — Privacy-safe continuity registry

Please add a portable continuity/reference contract that can ground generated
characters without embedding or uploading private photos.

Desired behavior:

- Stable character/entity IDs shared across works and versions.
- Age-at-scene, build, face/hair/clothing observations, confidence, provenance,
  and human-confirmation status.
- Local-only reference asset IDs/hashes and an explicit policy such as
  `provider_transfer: forbidden|approval_required|approved`.
- Prompts and provider handoffs receive approved textual observations by default,
  not the private source file.
- A provider package lists every asset/text field that would leave the machine
  before execution and blocks on missing approval.

Acceptance test: Amado Rosa can remain visually consistent from a local photo-
derived description while the photograph path is absent from an AI-provider
handoff.

## P1 — Variant lineage and review selection

Please make versions such as `v1`, `v2-photo-grounded`, and `v2-85pct` explicit
derivatives rather than filename conventions.

Useful fields/operations:

- parent manifest and transformation reason;
- changed dimensions: voice, pace, likeness, frames, captions, mix, or edit;
- private review candidate versus principal-approved version;
- separate Bertica and Herman findings without collapsing disagreement;
- command/report that identifies the latest review candidate for each scene
  without implying approval.

## P2 — Long-still motion quality controls

Our illustrated audiobook scenes use long cinematic stills. Please consider
manifest-level checks for:

- maximum uninterrupted hold and low-motion warnings;
- safe pan/zoom bounds that never expose blank canvas or crop a face;
- focal point and protected-region metadata;
- parallax/depth-layer hints without requiring a specific renderer;
- continuity checks for left/right position and eye line across adjacent shots;
- a no-lip-sync narration mode;
- clean A/B outputs for narration-only versus effects/music.

The goal is not constant movement. It is controlled, inspectable motion that
protects faces, captions, and the one emotional beat assigned to each frame.

## Recommended implementation order

1. Untimed planning manifests.
2. Speaker-aware cues plus protected pauses.
3. Audio conform/atomic retiming.
4. Source coverage and omission reporting.
5. Privacy-safe continuity/provider package.
6. Variant lineage and long-still quality checks.

## Cross-repo handoff request

Please respond with:

- which requests already exist in another form;
- the smallest schema/CLI slice you recommend implementing now;
- any BERTICA manifest you want copied as a fixture (prefer a sanitized textual
  fixture, not manuscript text, private photos, source voice, or binary renders);
- migration implications for the five current BERTICA manifests;
- the exact REEL artifact/command BERTICA should consume after the change.
