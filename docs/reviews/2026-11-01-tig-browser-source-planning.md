# Tig browser source planning

The native browser source planner resolves an authored logical resource map by
stable Tig IDs at one metadata checkpoint. Leaf revisions supply the exact byte
pins; a parent does not maintain a second mutable list of their hashes. This is
BROWSER-01 of [the browser integration plan](../tig-browser-composition.md),
with implementation entry `52afd03b7558713a87cc470918f94c7ecd0608d2`.

The API lowers controls to the existing browser manifest, computes scoped graph
requirements and source recipes, and freezes/restores metadata through existing
native context ports. The CLI preview opens no primary payload, starts no browser
and creates no output directory. It reports counts and opaque pins; failures use
a fixed diagnostic without source IDs or resource paths. Bounds in this planner
are admission limits, not held memory, capacity or execution reservations.

Local verification passes twelve browser-source fixtures, twenty shared delivery
template fixtures (four existing cases ignored), and two legacy browser validator
fixtures (one existing case ignored). Browser cases cover leaf-only invalidation,
alias/unique accounting, unsafe paths, definition and eligibility refusals, exact
private-review scope, named payloads, offline frozen metadata replay, one thousand
logical aliases, recipe projection and the actual metadata-only CLI. The final
warnings-denied all-feature/all-target Clippy, Tig-only library/scene-CLI check,
format check and Git whitespace check pass. This verifies metadata behavior;
neither browser execution nor private media correspondence was tested here.

Producer-applied Editor and Rights/Provenance checklists require preserving SVG
timing/geometry, exact source eligibility and immutable subjects. Storage and
identity checks distinguish aliases from physical objects, prohibit implicit
materialization, and retain frozen prior views. No selection, approval, provider
currency, custody, complete semantic coverage or eviction authority follows.
This is a producer review, not independent or human approval.

Sharing the private-review helper changes the containing source file bytes.
Existing context descriptors may therefore require their original qualified
runtime even where the helper's meaning is unchanged. Preserve versioned routing
and old tools; do not rewrite old evidence or demand replacement renders merely
to make a new tool accept an old context.

BROWSER-02 bounded resource delivery/frame streaming and BROWSER-03 managed
production/adoption remain open. The source recipe does not yet bind a finished
render's actual browser options, encoder parameters or clean-picture behavior.
It grants no output reuse authority. Hosted qualification, tiny real producer
fixtures, separately admitted private reference acceptance, normal lane entry and
rollback must precede production adoption. No active binding changes here.
