# Native scene reference bindings

The reference inspector accepts explicit `scene_contexts` and cache-backed
metadata locators. It reuses `reel-assembly`'s native scene resolver, preserving
its scope, canonical-cue coverage, template, policy, selected-binding and
ambiguity checks. See [the contract](../asset-references-v1.md).

Context nodes report document hashes, native fingerprints and selected binding
identities. Only native selected inputs receive `requires-asset` edges. Unused
binding entries remain observations; inventory membership stays separate.
Media presence is still size-only. A native resolution with missing selected
bytes is explicitly held. Neither result certifies rendered timing, source-text
or alignment payload contents, template-definition bytes or human approval.

All metadata passes the existing active-cache, read-budget and snapshot checks.
There is no cache discovery, media hydration, archive access, scene rewrite,
render, service or lifetime cache lock. Older repository-only profiles and saved
reference reports remain readable. Private acceptance data stays upstream.

Synthetic fixtures cover a bilingual authored scene, unused versus selected
binding changes, required-input absence, wrong scopes, candidate selection,
missing/ambiguous slots, wrong policy/template/source coverage, corrupted cached
metadata, descriptor/schema mismatch, bounds and changing input metadata. The
UI distinguishes native required inputs from inventory membership and shows
the saved registry generation. Review: [producer checklists](../reviews/2026-10-21-native-scene-reference-bindings.md).

Next: explicit source-authority and delivery-contract adapters, and independent
browser layout acceptance. A resolved scene is not a complete episode build.
