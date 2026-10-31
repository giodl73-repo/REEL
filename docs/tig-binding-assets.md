# Register current and historical binding assets

```text
reel-tig-register --binding-assets PRIVATE_PROFILE NEW_CANDIDATE_DIRECTORY
```

This adapter reads one exact cached `reel.scene-asset-bindings.v1` manifest and
projects explicitly mapped assets into independent `.tig` records. It works for
any project using that source format. It reads metadata only, creates new local
candidate files, and performs no media copy, Git adoption, approval, render,
cache initialization or removal.

Use profile schema `reel.tig-binding-assets-registration.experimental-1`:

| Field | Meaning |
| --- | --- |
| `source` | Exact manifest Object (`sha256`, `bytes`) already in the asset store |
| `scope_id` | Independently admitted source scope, which must match the manifest |
| `byte_source` | Explicit existing providers and read protection |
| `entries` | Current source binding key → target mapping |
| `max_output_bytes` | Positive total candidate budget, at most 16 MiB |

Each entry has `id`, portable one-component `.tig` `filename`, `definition`
(exact definition/authored-schema FileRefs), `authored`, `history` and
`authorities`. `history` maps explicit retained slot names to original binding
keys. `authorities` lists original decision/evidence binding keys. There is no
timestamp-based selection or invented historical version identifier. The current
binding supplies the exact selected Object; every historical binding supplies an
immutable stream with its original name and selection state.

Normally every mapped current/historical source logical ID must equal the target
ID. If legacy versions embedded revision names in their IDs, provide `source_ids`
as a complete binding-key → original-ID map for the current binding and every
historical binding. Every declaration must match the source exactly. The target
then provides the explicitly chosen stable canonical identity. Partial mappings,
wrong source IDs and accidental renames refuse. Original names remain in stream
provenance and the canonical map; source bytes and immutable subjects do not change.

The manifest itself is retained as a `source-bindings` evidence stream, alongside
the current and historical payload pointers. Original authority pointers remain
evidence, with `human_approval: not-reasserted` and `native_build: not-established`.
An imported delivery does not become a primary input or a newly qualified producer.

Source manifests may be larger than a native record: this adapter uses the
existing bounded registration reader (4 MiB per document, 16 MiB aggregate) and
rejects duplicate asset keys beyond its 4,096-key bound. The exact large source
stays asset-side. Tig's canonical/native record limits remain unchanged. Profiles
are bounded to 128 KiB and 128 target entries; each entry has at most 256 history
slots and 128 authority references. `current` and `source-bindings` are reserved
slots. Duplicate target IDs or case-insensitive filenames, path escapes, missing
mapped bindings, contradictory lengths and definition/schema mismatches refuse.

Existing destination directories are never overwritten. Validate all candidates
and their expected count before adopting any files into a project. A publication
failure can leave an unadopted partial candidate directory; use a new destination
for a retry. Stdout contains counts and authority flags; errors omit private paths,
IDs and hashes. Requests, provider paths and raw reports stay outside project Git.

Named byte verification, primary-input closure, producer freshness, independent
custody, publication and consumer cutover are separate checks. Registration reports
`payloads_verified:false` and `authorizes_eviction:false`.
