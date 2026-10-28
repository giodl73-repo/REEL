# Explicit canonical identities for scoped legacy inputs

Entry REEL `e749854beee75d6053bd197f361bdd0170260b7f` (draft PR221;
PR220/221 qualification and merge remain prerequisites). Tig WP-010/011/016,
IF-010/011. Actual private registration exposed two current scene-scoped inputs
with different bytes and the same legacy logical name. Neither hash is inferred
to be newer or preferable. Both source scopes and immutable subjects must survive.

Allow caller-declared `primary_ids` and `language_primary_ids` alias-to-canonical
ID maps in the existing registrar. Defaults preserve original IDs and candidate
bytes. Explicit mappings retain original native logical IDs, binding keys, scope
and source pins as provenance; they never change selected bytes, status or native
scene content. Canonical IDs remain opaque identifiers, not output paths. The
parent records the admitted map. No automatic namespace suffix or timestamp rule.

Reject unknown aliases/languages, invalid IDs and canonical identity collisions
with inconsistent bytes/status/annotations. Language overrides remain explicit;
shared native aliases inherit a base identity mapping only for the same original
asset, not for a different language variant. Preserve all parent references.

L0 fmt/Clippy/default isolation; L1 identity mapping, provenance, conflicts,
malformed maps and legacy byte equality; L2 full registration regressions plus
actual 22-scene reconciliation/schema/graph checks outside ordinary Git. Do not
adopt a contradictory global snapshot. Exact-head Linux/Windows/FFmpeg, upstream
qualification and whole merged-tree equality are required before squash merge.
Producer editor/sound/rights review only; no independent/human acceptance,
creative approval, runtime installation or storage/custody authority.

Initial qualification passes all thirteen native registration cases (0.42
seconds), four package cases (0.75 seconds), four versioned cases (0.30 seconds)
and locked all-target/all-feature warnings-denied Clippy (39.75 seconds). Three
new grouped tests cover ten invalid maps, source provenance/subject preservation,
unchanged unrelated leaves, explicit shared-language inheritance and empty-map
byte equality. An additional actual-shape two-alias/one-canonical-ID case follows.
No project records or cache bytes have changed; actual repair and hosted
qualification remain open. Producer disposition: pass_with_risk for explicit
namespace admission, not new media/creative approval.

Final local rerun passes fourteen native cases (0.33 seconds), four package
cases (0.25 seconds), four versioned cases (0.08 seconds), and all-target/all-feature
Clippy (0.63 seconds). The added shared-alias case confirms the same canonical
scoped asset is emitted once, with both parent aliases and the original native
identity retained. No production source changes follow that qualification.

Actual source `f873c60` prepares twenty-two scene sets with two explicit scoped
identities. All 714 candidates pass their authored schemas and have 714 distinct
IDs, with zero collisions. Comparison against the earlier conflicting candidates
changes exactly two leaves and two parents. All selected objects, definition pins,
statuses and history/control streams remain unchanged; unrelated leaves are
byte-identical. The two mapped leaves retain their exact original logical name
and source scope. No primary media is read, copied or rewritten. Raw mapping and
proof stay private outside Git. This resolves candidate identity consistency,
not renderer parity, production freshness, approval or storage custody.
