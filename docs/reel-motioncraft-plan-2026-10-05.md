# REEL Motioncraft

Date: 2026-10-05. Status: revised v4 after simulated repository-role review
and CAIMITOS consumer-boundary audit;
execution active on `codex/motioncraft`. Completion remains unproven.

## Objective

Make motion intent explicit and inspectable: compile shot-level entrance,
settle, hold, and exit direction into frame-based evidence, then demonstrate
better hierarchy, rhythm, and legibility in one 12–15-second explainer.

## First proof

Create an original 15-second motion-graphics explainer: **Why a queue grows**.
Audience: a general technical viewer watching on a desktop or phone. Use simple
tokens, a queue, and a worker; no external product interface or source canon.

- 0–3 seconds: ask why a queue grows when arrivals exceed processing.
- 3–11 seconds: show accumulation, then show arrivals slowing so the worker
  can catch up. Label the relationship without inventing measured statistics.
- 11–15 seconds: land on the takeaway: control arrivals or add processing
  capacity. Hold the causal relationship long enough to read.

These are initial editorial allocations, not universal timing rules. Freeze
the final script and timing before the matched comparison. Review asks what
caused accumulation and what changed to let the queue shrink.

## Design boundaries

- Keep production manifests unchanged by default. First audit production
  binding, choreography, sprite tracks, exposure sheets, motion-check, and
  review-pack seams. Reuse existing representations; add only missing intent
  in an opt-in versioned sidecar or extension with explicit compatibility.
- Record stable element IDs, motion purpose, focal priority, optional phase
  spans, reading holds, typography roles, palette roles, and delivery profiles.
  Phases can be absent: a static diagram needs no entrance or exit.
- Use declared zero-based inclusive working-frame spans and document mapping
  to delivery frames. Reuse exact production-duration arithmetic and expose
  rounding residuals; do not silently retime audio or shots. Enforce existing
  alignment tolerances and fail unsupported conversions explicitly.
- Begin with existing supported curves, clamped to their authored spans.
  Linear motion remains valid when intentional. Defer springs and overshoot
  until they have a bounded sampler and reviewed geometry.
- Distinguish declared stationary spans from unexpected freezes using existing
  motion checks. A reading hold can keep text still while another element moves;
  do not misrepresent an element hold as a whole-frame freeze allowance.
- Safe areas are explicit profile inputs, not an assumed universal social
  percentage. Reuse protected regions and caption-band reservation. Reflow
  landscape and portrait layouts rather than crop one master.
- Reuse audio event/beat bindings for reveal and cut cues, plus intentional
  silence. Keep captions and narration complementary; no mandatory effect on
  every entrance, fixed audio lead offset, or beat-aligned cut requirement.
- Technical checks evaluate structure and declared geometry. They cannot
  establish taste, comprehension, or creative approval.

## Implementation slices

### A — Author and compile motion intent

Produce a reuse/gap map and the smallest compatible contract. Compile exact
shot/element/phase identities and frame mappings into a deterministic plan.
Validate stale hashes, unknown IDs, invalid spans, incompatible overlaps,
duration mismatch, and declared geometry leaving safe bounds. Allow intentional
overlap between different elements; do not reject all concurrent motion.

Acceptance: valid complete and static fixtures pass; invalid fixtures fail with
specific diagnostics; old manifests retain current behavior. Hash the exact
bytes parsed. Do not require final art for planned intent or claim art bytes
were verified when they were not.

### B — Consume the plan and build review evidence

Wire one existing local preview/render path to the compiled direction using
the supported motion/shape/sprite vocabulary. A validator-only implementation
does not complete the goal. If the existing path cannot render the proof,
document the exact gap and choose a minimal local bridge before proceeding;
do not silently expand into a browser/DCC platform.

Extract phase start/end, settled pose, hold midpoint, adjacent boundary frames,
and transition-overlap samples, deduplicated and bounded to valid frames.
Add a uniform contact sheet for unsampled intervals, normal-speed playback,
and quarter-speed inspection of suspicious motion. Include frame index,
timestamp, shot/element/phase IDs, source hashes, render hash, and tool versions.
Samples support review; they do not prove every frame is defect-free.

Acceptance: samples match actual rendered frame indices, held intervals remain
correct, extraction is repeatable, partial failures do not publish success,
and reports preserve existing no-clobber and lineage conventions.

### C — Demonstrate and evaluate

Produce baseline and revised renders with identical script, claims, assets,
duration, audio sources, and output profiles. The baseline is a competent
existing workflow, not a deliberately broken render. Differences in layout,
motion timing, and cue placement are declared and tied to the direction plan.

Profiles: 1280×720 and 720×1280 at 30 fps (450 frames for 15 seconds), subject
to existing renderer bounds. Review at desktop and phone display sizes. Include
sound-on, muted with captions, and a restrained reduced-motion treatment.
Reduced motion preserves information, reading time, and the causal explanation.

Use original shapes/text and declared local fonts/audio. Reference articles
inform traits rather than supply copied compositions. Local previews require
no generation service, upload, or publication. Keep binary renders out of git.

Acceptance: technical tests pass and all requested review artifacts exist.
Apply the core `.roles` panel and REEL rubric to both variants using the same
questions and dimension scores. Label simulated reviews accurately. Record
actual viewer comprehension only if a viewer participates; do not invent it.

Quality target: revised Rhythm and Legibility scores each exceed baseline,
with no Emotion or Execution regression and total at least 60/100. Cite frame
and playback evidence for each score change; this is an evaluation target, not
an automated truth. Revise once if the comparison fails. If it still fails,
retain the evidence and report the unmet quality criterion; do not mark the
goal achieved. Human creative selection remains separate.

## Deliverables and completion

### D — Assemble and consume from CAIMITOS-shaped repositories

Cross-repository assembly is part of the goal, not a deferred integration.
The user explicitly requires usable CAIMITOS adoption for future and existing
episodes. Completion therefore includes the consumer adapter and the actual
scene-engine execution path, not only REEL contracts or a standalone preview.
CAIMITOS's current Rust adapter maps source/authority/planning records into
REEL's semantic graph and explicitly does not render or invoke FFmpeg. REEL
owns semantic selection closure, delivery validation, and rendering. Preserve
that ownership: consumers author intent against stable scene/shot/element IDs;
REEL validates, compiles, assembles, renders, and produces evidence.

Audit the current semantic assembly/delivery contracts before choosing the
attachment seam. Bind motion intent to an immutable selected graph revision
and its exact shot identities, not a mutable current pointer alone. Keep
consumer source IDs and language-local cue/sample clocks intact. Planned
motion may be validated before selection, but delivery must reject unresolved
or stale selected assets. Do not create a second source-selection system.

Assemble one package containing authored intent, compiled plan, exact manifest
or semantic delivery input, asset references, captions/audio, rendered media,
and review evidence. Reuse `reel.production-package.v0.1` inventory and receipts
where compatible; inspect the actual supported component kinds before adding
motion-specific kinds. The package is an integrity inventory, not an assembly
engine. Provide a documented consumer command sequence that demonstrably
reaches REEL's assembly/render path, with no hand-edited intermediate timing.

Use an original synthetic CAIMITOS-shaped fixture based on the existing
sanitized scene-delivery canary: two 48 kHz cues of 48,001 and 47,999 samples,
two picture attachments, a sample-offset effect, and intentional music silence.
Add optional motion direction and prove exact total 96,000 samples, preserved
cue/effect anchors, frame mapping residuals, and unchanged selected asset IDs.
At 24 fps this two-second fixture delivers 48 frames; sample identity must
not be rounded to the visual frame clock. Include a restrained illustrated
hold/push treatment to prove the contract is useful beyond kinetic explainers.

Acceptance:

- Trace authored episode defaults and scene/shot overrides through the CAIMITOS
  compiler/adapter, selected semantic revision, delivery input, existing scene
  renderer, and `scene-delivery-check`. Every required intent must be consumed
  or rejected as unsupported; no silently dropped direction fields. Confirm
  the actual schemas and command interfaces before implementing new fields.
- Deliver and test a new-episode authoring example and an existing-episode
  upgrade procedure. Upgrade by explicit versioned overlays/revisions bound
  to existing scene, shot, cue, and selected-asset identities. Preserve the
  old revision and rollback route; no source rewriting, automatic reselection,
  or retiming of accepted narration. Missing direction keeps legacy behavior.
- Test inheritance and explicit override precedence at episode, scene, and
  shot level, along with unknown/unsupported fields and contradictory intent.
  Prove both new-authoring and existing-revision upgrade through the consumer
  adapter into a rendered, assembled scene, not just parser tests.
- Implement required CAIMITOS adapter changes in one designated integration
  lane following its repository/session rules. Include tested invocation,
  version compatibility, and deployment/adoption instructions so CAIMITOS can
  use the completed capability. A synthetic canary remains the mandatory
  end-to-end proof; it does not require changing a selected live episode.

- With direction absent, the consumer canary retains its established semantic
  timing and media choices. With direction present, the assembled render
  exhibits the declared motion and its extracted review frames match the plan.
- Stale graph revision/hash, unknown shot IDs, wrong-language cue binding,
  missing asset bytes, and timing drift fail before delivery publication.
- A clean consumer workspace can hydrate and verify package assets, execute
  the documented sequence, and reproduce the compiled plan and semantic timing.
  Compare decoded evidence or declared tolerances for renders; do not promise
  byte-identical encoded MP4 across runtimes.
- Provide a cache hydration manifest with stable logical IDs, hashes, byte
  counts, provenance, and selection state for CAIMITOS handoff, using its
  `cache://sha256/` lifecycle. REEL stays cache-provider-neutral; resolve cache
  locators at the consumer boundary into verified package-relative files.
- Keep private family media out of REEL fixtures. Test the consumer route with
  synthetic media. A real CAIMITOS shot pilot is optional and must enter one
  designated project lane, follow its session instructions and asset authority,
  and preserve author/editor/visual approvals; it is not required for the
  synthetic integration proof.

Read-only audit sources (2026-10-05): CAIMITOS `AGENTS.md`,
`PROJECT_CONTEXT.md`, `WORKSPACE.md`,
`tools/caimitos-reel-adapter/README.md`,
`tools/video/reel_scene_delivery_adapter.py`,
`tests/test_reel_scene_delivery_adapter.py`, and
`tools/admin/Test-BerticaReelCompatibility.ps1`. No CAIMITOS files or private
media were changed or rendered during planning.

1. Reuse/gap map, versioned contract documentation, and example authored input.
2. Validator/compiler and one consuming local preview path.
3. Targeted frame evidence, contact sheets, and exact artifact lineage.
4. Matched baseline/revised explainer packages for both profiles and the
   reduced-motion treatment.
5. Regression checks, meaningful boundary/failure tests, and a final `.roles`
   review with finding dispositions and baseline/revised REEL comparison.
6. Working CAIMITOS adapter-to-scene-engine integration, new-episode authoring
   and existing-episode upgrade guides/examples, synthetic canary, verified
   production package/receipt, hydration proof, and compatibility results.

Run focused tests for the changed modules, fixture CLI checks, and applicable
repository-required checks. Cover frame boundaries, conversion, unknown IDs,
stale binding, hold handling, geometry, sample extraction, determinism, and
legacy behavior. Run render smoke checks when the required runtime is available.
Missing dependencies or blocked checks remain explicit unmet criteria.

Completion requires all six deliverables, no unresolved blocking technical
findings, and the quality target above. Plan acceptance is not implementation
acceptance. There is no release, push, publication, or provider selection gate
inside this goal.

Deletion target: replace one fixture's duplicate handwritten motion-timing and
review-frame schedule with compiled intent consumed by render and review.
Remove it only after output equivalence for unchanged timing is demonstrated.

## Deferred follow-on goals

- Implement a production Remotion/browser adapter with pinned runtime, frame
  seeking, seeded randomness, font readiness, and local artifact contracts.
- Add bounded springs/overshoot, kinetic typography and diagram primitives.
- Expand benchmarks to title sting, animated diagram, and restrained editorial
  sequence, with evidence-based visual-direction presets.

These follow-ons are not required to finish Motioncraft's first proof.

## Review and incorporated feedback

[Role review](../signals/roles/check/reel-motioncraft-plan-roles-check-2026-10-05.md)
records MC-01 through MC-12 and retained disagreements. Revision v2 added a narrative
brief, optional phases and timebase rules, a required consuming renderer,
matched comparison, authored sound/silence, phone/reduced-motion review,
provenance boundaries, and falsifiable completion criteria. Revision v3 adds
consumer-owned intent, REEL-owned assembly, immutable semantic bindings,
sample-accurate canary tests, and cache-portable verified handoff.
Revision v4 requires actual CAIMITOS compiler/adapter and scene-engine
integration plus tested future-episode authoring and existing-episode upgrades.

## Goal-ready instruction

> Run REEL Motioncraft using docs/reel-motioncraft-plan-2026-10-05.md. Complete
> slices A–D: reuse existing contracts, implement compatible motion intent and
> a consuming local preview path, generate targeted review evidence, and prove
> the result with matched 15-second queue-explainer variants. Incorporate the
> recorded .roles findings, prove assembly and hydration with the synthetic
> CAIMITOS-shaped consumer canary, run appropriate checks, and report the final
> results for both new-episode authoring and existing-episode upgrades through
> the actual CAIMITOS adapter and REEL scene engine. Deliver the required
> consumer integration changes and adoption instructions, then report the
> artifacts and REEL comparison. Keep deferred browser, spring, and benchmark
> expansion work outside this goal. Do not claim completion while any required
> technical check, deliverable, or quality criterion remains unmet.

## Research basis

- [Pixel8 production retrospective](https://www.pixel8production.com/blog/opus-5-5-motion-graphics): first-party account of typography, layout, transition, and pacing revisions; a small self-reported sample, not a general benchmark.
- [Community design rules](https://github.com/haidrrrry/claude-remotion-skill/blob/main/remotion-motion-graphics/references/design-rules.md): useful hierarchy and inspection prompts; stylistic prescriptions remain optional.
- [Remotion animation documentation](https://www.remotion.dev/docs/animating-properties): frame-driven animation as the rendering foundation.

No model-provider performance claim is required for this project.
