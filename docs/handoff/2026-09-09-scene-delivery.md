# Scene delivery consumer handoff

Purpose: eliminate duplicate consumer timing calculations and make exact media
consumption visible in a reproducible scene result.

Entry: docs/scene-delivery-v0.1.md. Commands: scene-delivery-plan,
scene-delivery-render, scene-delivery-check. Contract: reel.scene-delivery.v0.1.
Adoption must pin the merged REEL commit; the Cargo version alone is insufficient.

## Consumer sequence

1. Run the synthetic scene_delivery tests including the explicit FFmpeg canary.
2. Export one selected scene to the strict job with a hash-bound cue contract and
   production manifest. Hydrate all required source/receipt bytes; preserve native
   audio and original phrase alignment. Do not translate old seconds into anchors.
3. Declare actual D/M/E choices. Required unavailable music/sonics are held, not
   automatically silent. Identify earlier score deliveries absent from the selected
   master and request the composer's exact current placement decision.
4. Render the same selected material and verify picture/action order, native audio,
   frame partition, samples and final master against the existing scene. Run audio
   and existing VFX/caption checks. Explain every intended difference.
5. Replace one authorized native cue in a separate canary, supply its new phrase
   alignment, and prove dependent pictures, effects, titles and captions move from
   those anchors while unrelated scene outputs retain their hashes.
6. Use the existing changed-only dependency graph for per-scene jobs and an ordered
   episode-conform node. Pin the executable, FFmpeg build and selected inputs in
   graph recipes. Never hard-code a list of version-number substitutions.
7. Prepare explicit long-composition revisions or decision-backed stillness
   exceptions. Avoid using micro-crops or universal motion to satisfy a validator.
8. Keep D/M/E and high-quality picture intermediates through episode assembly;
   encode the delivery once. Review derivatives never become master inputs.
9. Bind one current master selection per language and purpose, with predecessor,
   input plan, receipt and review status. Newest filename is not selection.
10. Return the exact pushed adapter commit, merged REEL pin, synthetic/parity/recast
    evidence, input/output hashes, cache hydration manifest, and remaining owners.
    The coordinator reviews the result before closing the request.

## Limits to preserve

External layers are declared and their evidence verified, but this tool does not
certify that they were composited. Existing effect/animatic/browser/caption tools
own that proof. This bridge does not generate new motion, align new speech, author
emotion, choose a musical cue, replace performer consent, or grant release approval.
No private consumer media or literary text is included in this repository.
