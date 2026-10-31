# Versioned source evidence JSON boundary

The broader source-schema migration found a valid legacy scorecard record whose
unprojected metadata contains decimal numbers. Versioned registration previously
parsed the entire external document as Tig canonical identity JSON and refused
it. That conflated an immutable evidence blob with the small native projection.

Only the raw source-record parser changes. It accepts ordinary finite JSON
numbers, rejects duplicate keys at every depth, and retains the existing 64 KiB,
32-level and 4,096-node limits. The exact original bytes remain the source
evidence object's SHA-256/length subject; parsed decimal values are never used
to rewrite that evidence or compute a semantic identity.

The adapter still projects only declared identity/current/version/release fields.
Byte pins remain typed `Object` values; fractional, negative or unsafe native
lengths cannot produce an admitted instance. The resulting Tig instance and its
definition/schema binding still use Tig's unchanged canonical identity rules.
Profiles, definitions and other native documents keep their existing parsers.

Regression coverage includes decimal/exponent/negative-zero metadata, precision
that cannot round-trip through a floating representation, unchanged exact raw
source pins, native output admission, nested duplicate keys, non-finite numeric
overflow, excessive depth/nodes and invalid byte pins. Existing family, history,
lock, release, no-clobber and fixed private-error tests remain in the test target.

Producer-applied rights/provenance review: original metadata stays private and
byte-pinned; no scoring, approval, selection, publication or custody is inferred.
Raw source content does not enter diagnostics or the native authored projection.
This is an adapter correction, not a relaxation of Tig identity or an independent
review. Hosted qualification and production adoption remain separate.

Executed: all six versioned-registration tests and all four package-registration
tests pass; strict all-target/all-feature Clippy and whitespace checks pass. The
real refused scorecard and remaining source batch now register locally without
rewriting evidence or copying media. The earlier 271 accepted candidates remain
untouched; the corrected local tool produces the final 18 of 289 candidates.
Independent comparison of all 289 candidates preserves 550 exact version-package
references. These are registration checks, not full native authoring or delivery
acceptance; raw per-record results remain outside Git.
