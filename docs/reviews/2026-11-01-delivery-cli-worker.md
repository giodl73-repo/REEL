# Delivery capture and contained worker

Entry `1f3b22e6445b9e0ea0c9778583665edaf78b2ab0`, tree identical to the
previous managed SDK candidate. CLI scope: explicit request-family dispatch and
delivery mode over the existing capture/coordinator/worker/planning/records paths.
No alternate job driver, cache, source owner or process-containment mechanism.

The worker selects scene or delivery only by its known request schema and checks
the context mode before source/provider access. Legacy scene request serialization
remains unwrapped. Delivery planning and render output-set preflight use the same
memory control provider as SDK execution; record proposals derive input-definition
exclusions from the actual request family. Non-compiled preparation accounting
applies to delivery mode, with no preparation-cache directory.

Capture now accepts a separate delivery capture schema, validates its bounded
request before stores, reserves metadata capacity and finishes/reopens the exact
context. It requires no preparation cache and allocates no preparation directory.
Scene/episode capture retains its existing required preparation binding. Small
authored controls and large selected objects retain the same shared selectors.

Producer concurrency/storage/privacy/editor review: worker tickets still bind
exact admission bytes, recipe and runtime; mismatched families/modes and foreign
metadata refuse. Output caps are checked before rendering and after production.
All source enrollment, resource authority, result retention and Git proposal work
reuse existing code. Structured worker results still assert no approval or
current external publication. Actual private-project acceptance remains open.

The new fixture captures via the real CLI, verifies metadata-only capture and
context equality, then reuses the full native CLI lifecycle with a delivery
request. It exercises budget refusals, dynamic planning, contained rendering,
retention, record candidates, original-source unavailability and Git handoff.
Historical PRs remain evidence for their original source, not retroactively for
this new worker. No installed production runtime or project binding is changed.

Local results: all 22 native-worker/capture/planning/retention unit cases pass.
The complete new delivery capture/CLI lifecycle passes, including retained record
candidates and Git handoff; the existing compiled-scene capture/CLI lifecycle
also passes. All-target/all-feature warnings-denied Clippy passes. Actual media
project semantics, hosted qualification and installed-runtime cutover remain open.
