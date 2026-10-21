# Asset schema and reference inspector

The generic Rust asset tools now export inspection contracts, inspect explicit
metadata roots, query direct incoming/outgoing links, and generate an offline
browser view. See [the contract](../asset-references-v1.md). The native
`reel-assembly` episode, scene, binding, policy and template parsers remain the
authoring grammar. The catalogue explicitly distinguishes exported structural
projections from native contracts without JSON Schema exports.

Records, immutable version locks, package assets, episode inventories and parts
form a typed graph. Membership never becomes a required build dependency.
Previous versions remain visible and registry current state is separate from a
record's current pointer. Unsupported fields are coverage findings. Raw reports,
JSON media and source-path text remain opaque; nothing mines them for hashes.

The inspector hashes named metadata, checks media size/presence only, and does
not read archive roots. Corrupt first-tier metadata holds that target. Traversal
has explicit budgets; source/configuration/registry changes reject the snapshot
without a shared lifetime lock. Saved reports have an integrity digest, and
outputs publish atomically without overwriting existing files. A strict check
exits unsuccessfully on any finding after writing its private report.

Synthetic tests cover shared inventory membership, previous Goldens, parts,
corrupt metadata, opaque JSON traps, missing/unknown contracts, exact lock
mismatch, dependency cycles, snapshot changes, duplicate target expectations,
path limits and report tampering. Browser-script tests cover filtering,
navigation, relationship meaning, literal text and pagination. The real private
episode pilot and all its metadata stay in the producing project/cache. Neither
that pilot nor browser-script execution establishes visual browser acceptance
or native episode build readiness.

Next increments are explicit adapters for additional families/authority and
delivery contracts, complete native JSON Schema exports and native semantic
resolution. Do not replace these gaps with arbitrary recursive JSON traversal.
The graph supplies inspection, not cache removal authorization.
