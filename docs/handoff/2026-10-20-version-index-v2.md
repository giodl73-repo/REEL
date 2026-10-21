# Version-index/protection checkpoint

Added a compatible one-base/patch control reader, explicit `compact-versions`,
metadata-root protection, read-only explicit legacy consumer scope and
`consumer-protections`. Flat states, immutable shards, existing CLI and local LFS
contracts remain readable. Default redundant-copy verification can run with
active jobs; actual apply still refuses live/unreconciled admissions.

Executed locally: 80 asset-crate tests (79 unit plus one real-executable CLI regression), Clippy with warnings denied, release build,
formatting, roles and whitespace. Six new cases cover representation/state
preservation, corrupt/unauthorized/recursive bases, invalid patches, protected
metadata roots and read-only legacy consumer behavior. Public inputs are synthetic.

Private consumer proof is upstream: a complete protection export including an
older indexed consumer succeeded without changing that checkout; explicit
producer compaction preserved semantic controls and native current/history
lookups. No production cache removal or publication authority is inferred.

Producer-applied rights/provenance review: no private values, paths, reports or
assets copied into public fixtures. Toolchain adoption is explicit and must not
replace a running executable. Participating old runtimes require compatible
admission behavior or quiescence before destructive maintenance.

The CLI regression covers the executable main-thread stack boundary. A streaming
hash buffer is heap allocated so Windows main stacks do not overflow even when
library test-worker stacks are larger. No real apply was requested by the failing
private verification case; the fixed command must be rechecked before adoption.
