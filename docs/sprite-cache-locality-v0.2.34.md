# Recipe-local sprite cache invalidation (v0.2.34)

REEL raster keys now depend on the effective source layers for one materialized
character request rather than the hash of the entire recipe catalog. A source
fingerprint covers the recipe and slot identities, transform stage, verified
source-image hash, effective mirror behavior, and transparent-layer markers.

This preserves deterministic addressing while narrowing invalidation. Editing
one pose recipe produces a new cache entry for its consumers, but unrelated
requests can reuse their verified existing entries. The materialization receipt
records each output's source fingerprint and still pins the complete catalog
hash for provenance and reproducibility.

The v0.2.34 CLI test fixture changes one goalkeeper recipe in a three-request
plan. The two skater requests are reused and only the goalkeeper request is
written. A first run after upgrading the cache-key algorithm creates new keys;
subsequent recipe edits receive the localized behavior.

Physical cache roots remain runtime inputs. Neither the source fingerprint nor
the portable receipt records a machine path.
