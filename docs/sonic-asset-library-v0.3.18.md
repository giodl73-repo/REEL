# Governed sonic asset resolution in CLI v0.3.18

REEL resolves declared sonic-asset intent. It does not choose a hero sound,
approve a candidate, infer a vehicle or instrument, generate audio, clear a
license, or grant creative or release approval.

## Contract and commands

The strict `reel.sonic-asset-catalog.v0.1` catalog carries stable asset and
variant IDs, authority state, an optional authority-receipt hash, license facts,
transitive lineage hashes, a local locator, exact source hash and byte count,
PCM WAV geometry, and optional loop and sync metadata. A catalog may also
declare versioned approved pools. Pool member order is part of the contract.

The strict `reel.sonic-asset-request.v0.1` request binds the exact consumer
manifest hash and one selection rule per audio-event ID:

- `exact` names an externally selected asset and an unambiguous variant;
- `approved-pool` names a pool/version and stable selection key.

Resolve and independently recheck a packet:

```powershell
reel sonic-assets-resolve catalog.yaml request.json `
  --resolution resolution.local.json --receipt resolution.receipt.json

reel sonic-assets-check catalog.yaml request.json `
  resolution.local.json resolution.receipt.json --output json
```

`resolution.local.json` is deliberately non-shareable because it contains
resolved machine paths. `resolution.receipt.json` is path-free. It binds the
catalog, request, consumer manifest, resolution, tool version, chosen logical
IDs, source hashes and byte counts, geometry, authority state/receipt, license
ID, and lineage. Both state that the operation neither selects creative output
nor grants approval. Existing outputs are never overwritten.

Materialize the checked resolution into a machine-local manifest:

```powershell
reel sonic-assets-materialize-manifest catalog.yaml request.json `
  source-manifest.yaml resolution.local.json resolution.receipt.json `
  --output-manifest manifest.resolved.local.yaml `
  --output-receipt manifest.resolved.receipt.json

reel animatic-audio-render manifest.resolved.local.yaml `
  --asset-root . --output review.m4a --stems-dir stems
```

Materialization re-runs catalog/request/source verification before replacing
the named events' `source` values. The output remains an ordinary REEL manifest;
the source locator is an execution detail, never asset authority. Its path-free
receipt binds both resolution artifacts and the source/output manifest hashes.
The existing renderer then binds the materialized manifest and current source
hashes and emits pre-master D/M/E, pre-master sum, mastered full mix, no-score,
mono, and small-speaker variants using its established receipts.

## Authority states

Production exact resolution accepts `selected-private-production`,
`principal-approved`, and `release-cleared`. An approved-pool request accepts
`approved-pool` plus the two stronger reviewed states. `candidate`,
`diagnostic-placeholder`, and `superseded` always fail closed. `fixture-only`
is accepted only when the request explicitly declares `engineering_fixture:
true`; it cannot silently enter a production request. A production request also
rejects a license record whose `permits_production_use` is false.

Deterministic pool selection hashes the exact catalog hash, pool ID/version,
request ID, and selection key, then indexes the declared ordered membership.
Runtime randomness is not used. Any catalog or pool membership change changes
the bound catalog hash and requires a new resolution packet.

## Audio validation

Every selected source must be an integer PCM RIFF/WAVE file. REEL measures the
sample rate, bit depth, channels, and samples per channel and requires exact
agreement with the catalog and any request-specific constraints. It also
rejects missing sync markers, missing or invalid loop regions, stale hashes,
wrong byte counts, ambiguous variants, duplicate IDs/bindings/members, unknown
members, unapproved pool members, source tampering, packet tampering, and output
overwrite.

48 kHz, 24-bit WAV and mono point sources are production policy expressed by a
consumer request, not an implicit creative default. Stereo remains valid when
the request and catalog explicitly require it.

## Privacy and boundaries

Catalogs and local resolutions can contain private locators and are private by
default. The resolution and materialization receipts contain no path or
filename. Logical IDs may still be sensitive and should be governed by the
consumer repository's disclosure policy.

REEL does not download models, call providers, upload source audio, generate
variants, select a Golden, or decide whether a technically valid sound belongs
in a scene. A successful check proves deterministic technical resolution of
already-declared authority; it is not listening review or publication approval.
