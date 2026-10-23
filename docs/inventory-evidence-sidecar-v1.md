# Persistent inventory evidence sidecars

Status: opt-in Rust library, `inventory_evidence::sidecar`. Derived metadata
format only; an explicit [existing-cache file backend](inventory-evidence-file-store-v1.md)
is available, without production adoption, catalog admission, CLI or planner
integration. It is not the universal asset-graph interchange format.

The existing source adapter verifies an admitted inventory once. Explicit
`publish(store, &prepared, limits)` writes immutable, content-addressed chunks
and returns the exact header descriptor. `lookup(store, &header, &expected_source,
subject, limits)` reads only the header and the relevant radix path. Reopening
the store does not require the original source reader or resident preparation.

```text
admitted original source -> verified preparation -> explicit immutable publication
                                                        |
caller-admitted header hash -> source-bound header -> radix branches -> leaf
```

## Identities and evidence

Every stored address is ordinary SHA-256 of the exact stored bytes, with an
exact byte count. Schema identifiers are stored in the bytes. The private v1
codec uses compact sorted-key JSON with serde_json encoding; reads reject
noncanonical, duplicate-key, unknown or omitted wire fields. Changing this codec
requires a format version change, not a silent reinterpretation.

The header binds encoded and decoded source identities, explicit encoding,
source schema, project, authority namespace and adapter ID/version. It also
retains profile identity, registry generation, optional capture interval,
scope, named scope, producer dependency coverage, tier observations and counts.
Lookup requires the caller's expected source and rejects a mismatch before
reading tree nodes. Source configuration/runtime admission remains external.

Rows retain original source record ordinals. A result's `source_record` locates
`/objects/<ordinal>` in the admitted original decoded document. That is
provenance, not an independent inclusion proof against a monolithic source.
An attacker able to admit a fabricated derived root can fabricate its contents;
source binding is not proof that a trusted transformation actually ran.

The caller must therefore admit the exact derived header from a trusted build
and retain the original source for recovery/audit. Admitting an arbitrary root
solely because its header names a trusted source is insufficient. The API
performs visited-path checks, not a global closure verification. Cross-leaf
counts, ordinal uniqueness and off-path integrity are established by the
trusted builder, not independently re-proved on each lookup.

Captured registration, physical measurements and size-match flags remain
observations. `None` means not listed at capture, not physical absence, zero
references or deletion permission. Missing/corrupt referenced chunks are errors.
No output is verified custody or current selection authority. The original
producer's dependency-coverage flag does not establish physical census coverage.

## Layout and limits

Branches route on the next lowercase SHA-256 hex digit, with at most 16 children.
Leaves split adaptively by row count and encoded byte size; depth cannot exceed
64 subject digits. Oversized single records or header metadata fail explicitly.
The header contains metadata rather than a flat list of every leaf.

| Budget | Build default | Lookup default |
| --- | --- | --- |
| Individual node bytes | 256 KiB | 256 KiB |
| Total encoded bytes | 1 GiB | 8 MiB |
| Nodes | 100,000 | 66 |
| Rows | 128 per leaf | 4,096 in visited leaf |

Node size has a 4 MiB ceiling; build leaf rows have a 4,096 ceiling. Read
descriptors, cumulative bytes and node counts are charged before store open.
Each read stops after the declared bytes plus one, then verifies exact identity.
All rows in a visited leaf are validated, including nonmatching rows, subject
ordering, duplicate ordinals, routing and copy/tier consistency. Byte bounds
apply before JSON decoding; row limits apply after the byte-bounded node is
decoded. These are not exact allocation/RSS bounds or a deadline guarantee.

Preparation still verifies/parses the full source into memory once. Build uses
bounded leaf attempts and field-length preflight, but is not streaming source
ingestion. No incremental source diff, write-amplification or large real-report
benchmark is claimed. Ordinal changes on refreshed sources can change many
leaves, even if their observations otherwise look alike.

## Publication, interruption and concurrency

The `Store` trait is explicitly supplied by the caller. An optional `FileStore`
uses named protected staging sessions and the existing CAS layout;
`ReadOnlyStore` makes exact queries without session recovery. There is no default
filesystem path, new cache, background task, archive scan or network resolver.
Implementations must provide immutable atomic no-overwrite publication and
reject corrupt existing winners. Library publication verifies the returned
descriptor and exact readback. Reads never silently rebuild a lost sidecar.

Children precede their parent; the header is last. The function never updates
a catalog or active root. Error returns no header descriptor, but may leave
unrooted immutable chunks. Builders need explicit staging protection, capacity
accounting and retirement policy for these chunks. Their cleanup is not automatic.
Successful publication is not root admission or authorization for a move.

There is no global cache lock. Identical builds can address identical nodes;
new source snapshots coexist with independently readable older snapshots.
Backend multi-process atomicity, filesystem durability and catalog CAS/revision
semantics remain separate adoption work. Local fixtures test persistence and
failure boundaries, not power-loss durability or process races.

## Privacy and verification

Chunks contain private paths, subjects and source identities. Store them as
private metadata assets under the project's normal custody policy, not raw
payloads in Git or shareable scorecards. Production public source/observation/
result types remain without automatic serialization; only private internal
wire types serialize chunks. No automatic shareable export is introduced.

`tests/inventory_sidecar.rs` uses isolated synthetic temporary directories and
a no-clobber file backend. It covers reopening after dropping preparation,
1,000-row selective reads, source refresh coexistence, duplicate publication,
stale identities, corruption, missing nodes, malformed semantics/routing,
publication interruption, false store success, byte/node/row budgets, oversized
metadata, full-depth prefixes and native census/named-report round trips.
All 25 sidecar tests pass, including complete-closure count/ordinal and missing
off-path checks. With 212 unit tests, five prior integration, 100 model,
23 source-adapter and 30 file-backend tests, the full suite passes 395 tests. Strict
all-target Clippy, formatting, ROLES structure and diff checks pass locally.
Fixtures read no project cache or private media and do not change runtime pins.

The backend now has bounded-staging, process-concurrency/termination fixtures
and optional [capacity admission plus verified handoff/retirement](inventory-evidence-handoff-v1.md).
Whole-index validation is explicit; ordinary queries stay selective. Remaining
adoption work: managed producer/collector integration, device/platform crash
tests, configuring the optional [revisioned root catalog](inventory-evidence-catalog-v1.md), cancellation/deadlines and
shadow comparison with existing planners. Receipt/registry adapters remain separate.
