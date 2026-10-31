# Planned primary inputs in generation advice

Entry: qualified REEL `65b2ac77bcf24d988433852ed23fe7e1459c8250`.
Scope: generation-set inspection, its existing synthetic CLI fixture and docs.
No storage provider, journal authority, renderer or active project is changed.

The workflow starts with a Spanish text asset, a planned English translation,
and a cue generated from that translation. Before the first translation exists,
the cue has no input byte identity from which to calculate a desired recipe.
Previously inspection attempted recipe capture anyway and refused the entire set.

Inspection now validates typed declarations and distinguishes unselected inputs
from selected inputs outside admitted states. It reports unavailable input IDs
privately, with null direct basis/recipe and blocked downstream advice. The
translation can retain its own unbuilt catch-up advice when its inputs are ready.
Ordinary unselected manual inputs also block. No fake hashes are introduced.
Strict recipe/context/replay admission remains unchanged.

Original claims still require admitted historical providers and successful
archive replay even when the current desired recipe is unavailable. Original
target ID and definition ID must match the current requested target. Blocking
cannot hide corrupt, unavailable or unrelated original evidence. Matching recipe
claims remain distinct from actual execution, selection and human approval.

Verification uses the existing real CLI fixture with primary objects offline:
planned translated text blocks its cue; a planned manual source blocks both;
corrupt/foreign original claims and selected-but-held inputs refuse without a
partial report. Existing matching, stale, missing-scope, unproven, cycle,
no-clobber and privacy assertions remain. Tests and strict Clippy must pass on
the final source; hosted qualification and runtime adoption remain separate.

Producer-applied role review: rights/provenance requires retained original
evidence, private IDs only in the private report, and no execution/approval claim;
editor checks the dependency order rather than suggesting a premature downstream
render. Findings: pass with these boundaries for metadata advice. This is not
independent review, model consent or production acceptance. Tooling owns actual
producer enrollment and project cutover; project owners retain creative choices.
