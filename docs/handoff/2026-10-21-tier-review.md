# Offline inventory-review checkpoint

`tier-review` groups a fresh saved plan with all-consumer protections into a
private offline view. `tier-request` validates an exact owner's block decision
and prepares the existing native dry-run request. It performs no cache mutation.
See ../asset-tier-review-v1.md for budgets, contract and explicit limitations.

Executed locally on Windows: 88 asset-crate tests (87 unit, one actual executable
regression), scoped Clippy, release build, formatting, role structure and whitespace
checks. Six new tests include cross-consumer changes while the primary plan stays
fresh and the real decision-to-native-verification route. Inputs are synthetic.
Private production proofs, raw inventories and browser evidence belong upstream.

Producer-applied review: observation is not backup integrity, metadata freshness
is not current physical state, block choice is not application, and source pin
adoption is separate from replacing an executable. Scope/decision mutation and
stale protections fail closed. No private hashes, roots, media or reports are
included in public fixtures. No human or publication approval is inferred.
