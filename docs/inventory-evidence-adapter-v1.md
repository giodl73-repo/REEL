# Read-only cache-report evidence adapter

Status: opt-in Rust library implementation, not a CLI/runtime adoption or a new
index interchange format. Module: `reel_assets::inventory_evidence`. Its first
supported source schema is the existing `reel.asset-cache-report.v1`, adapter
`reel.cache-report-metadata` version 1. No existing producer or planner is changed.

## Supported producer shapes

| Producer | Preserved distinction |
| --- | --- |
| `accounting::report` | Declared inventory scope; optional census interval; producer dependency-coverage flag |
| `accounting::observe_named` | Named-subject scope; absent capture interval stays unknown |

Identity JSON and explicitly declared gzip are supported. No encoding sniffing,
format guessing, generic receipt adapter or registry adapter is added. Unknown
schema/adapter versions fail. Existing v1 diagnostic fields are skipped without
interpreting them as authority; their original evidence remains bound by source
hash. Duplicate top-level/interpreted fields, subjects, tier IDs and canonical
copy tiers are rejected. Uninterpreted source fields remain in the original asset.

## Explicit preparation and queries

The caller supplies an admitted `Source`: encoded and decoded SHA-256/byte
descriptors, encoding, source schema, project, authority namespace and adapter
version. The source catalog and actual implementation/runtime pin remain the
caller's responsibility. A source document cannot authorize itself or invent
another authority namespace. The report's project must match the admitted source.

`prepare(reader, &source, limits)` verifies the encoded source, bounded decoding
and decoded identity before parsing. It reads only the provided reader. It never
opens reported paths, resolves CAS locations, enumerates storage, publishes
metadata, invokes a command, requests network access or changes a selection.
Encoded reads stop after the declared byte count plus one; gzip decoding also
stops at the declared decoded count plus one. All gzip members/trailers are
consumed; truncation, CRC failure, trailing junk and excess decoded data fail.

Preparation builds a memory-resident, immutable `PreparedInventory`, indexed by
exact subject hash. Subsequent `lookup` is an in-memory keyed lookup and does not
reparse the source. It returns either `Listed` captured metadata or
`NotListedAtCapture`, never current physical absence or removal permission.
Malformed query hashes are errors. `witness` provides the admitted source and
original `/objects/<ordinal>` locator, even when index key order differs.
The locator is provenance, not an independent inclusion proof.

Preserved fields include declared/unknown object size, captured registration,
copy tier/size, physical identity/measurement status, allocation, link counts,
size-match flag, captured tier roots/volume IDs, profile identity, registry
generation, declared scope and optional census interval. Wrong-size copies and
unmeasured identities remain observations rather than being silently repaired.
Contradictory size-match flags and invalid tier/identity references fail.
For registered objects the size-match flag compares against the declared size.
For unregistered objects the producer has no such expectation: its flag may be
true even when copies differ in observed size. The adapter preserves that fact,
and does not turn an unregistered row's summary size into descriptor authority.

The native producer's `coverage_complete` means dependency coverage; it does not
prove a complete physical cache census. `payload_hashes_verified`,
`removal_authorized` and selection-looking diagnostics never elevate this
adapter's output: its copy type intentionally contains no verified-custody or
removal-authority flag. Registration here is membership in the captured report,
not authorization by today's registry. Different tier paths/volume IDs do not
prove independent storage failure domains.

## Budgets and privacy

Caller limits cover encoded bytes, decoded bytes, object count and total copies.
Object limits reject before decoding the next object. Copy limits are checked
during array decoding after at most one additional bounded row; a row has at
most 64 copies and a report at most 64 tiers. No partial prepared index is
returned. The final validation also checks accumulated counts and identities.

Default budgets: 256 MiB encoded, 512 MiB decoded, one million objects and two
million copies. Defensive ceilings are 512 MiB encoded, one GiB decoded, one
million objects and four million copies. Larger sources require a future bounded
backend or explicit adapter revision; limits are never silently raised.

This is not constant-memory streaming or persistent sharded indexing. Encoded/
decoded source bytes and typed metadata consume memory during preparation; these
input/record limits are not an exact RSS bound. There is no deadline/cancellation
contract yet, and a caller-provided reader can block. Keep preparation explicit
and bounded; do not invoke it automatically during orientation or every lookup.

Source descriptors, witnesses, rows and tier data are private evidence without
automatic serialization. `summary()` is a separate serializable projection of
adapter ID plus object/copy counts only. No subject hashes, authority names,
paths or witness links enter that projection. API callers still must respect
privacy when inspecting or manually exporting private fields.

## Verification and remaining gates

The 23 adapter integration tests cover both actual Rust report producers in an
isolated synthetic filesystem fixture, exact row/copy/witness parity, identity
and gzip equivalence, multi-member gzip, CRC/truncation/junk/expansion failures,
byte/record/copy limits, duplicate records, malformed references, scope/adapter
rejection, private summary projection, unknown metadata and capture semantics.
The fixture does not access project caches or private media.

All 195 unit tests, five existing integration tests, 100 model scenarios and 23
adapter tests pass locally. Strict all-target Clippy, formatting, ROLES structure
and diff checks pass. This establishes a real-format read-only adapter, not a
benchmark, real private inventory acceptance, complete evidence algebra or safe
demotion integration. Persistent source-bound shards, receipt/registry adapters,
time/cancellation support, catalog admission, crash/concurrent publication and
shadow planner parity remain follow-up work before runtime adoption.
