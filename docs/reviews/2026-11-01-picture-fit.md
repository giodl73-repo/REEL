# Explicit picture fit producer review

Entry `d440986ca0c13880ad72b2a15db2edd9c2db4e7f`; scope and L0/L1/L2
controls are in [the contract](../picture-fit.md). Actual imported cel headers
have an aspect ratio slightly different from their retained delivery canvas;
the retained producer explicitly stretches them. Native contain-and-pad must
not silently be presented as equivalent framing.

Editor lens: fitting is an explicit picture parameter. Default/explicit contain
keep identical decoded picture/master frames; stretch fills the declared region
after optional crop. Motion recipes reject stretch rather than silently ignoring
one of two geometry controls. Existing canvas/region limits still apply.

Identity/privacy lens: the consumed source object is unchanged; a fit edit changes
the authored job/recipe and rendered output. It grants no selection, creative or
publication authority. Independent primary-reference replay reads the same
declared geometry choice. Omitted fields preserve existing JSON serialization;
Rust struct literals require the new optional field, documented in the contract.

Local validation: the wide white fixture renders default, explicit contain and
stretch and checks known edge pixels, full default/contain decoded equality,
native checker and primary-reference correspondence. All 31 regular delivery
cases, separate native legacy sample/stem/frame/tamper case and locked all-target/
all-feature warnings-denied Clippy pass. Formatting and whitespace checks pass.
This is producer review, not independent/creative approval or actual project
correspondence. No installed runtime or selected media changes.
