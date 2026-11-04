# Associate retained checksums with saved target metadata

`reel-tig-associate-index PROFILE NEW_PRIVATE_JSON` joins an exact retained
checksum index to an exact saved inventory. Build with `--features tig`. The
result is an explicit **index-backed association**, never a provider receipt or
proof of what a remote target currently contains. It can feed the existing
[delivery-table registrar](tig-delivery-table-registration.md), retaining both
original inputs as provenance on each resulting artifact.

The private profile schema is
`reel.tig-checksum-index-association.experimental-1`:

- `byte_source`: existing Tig store layout/protection/read budgets. Nothing is
  initialized. Metadata and optional payload reads obey the same provider limits.
- `index`, `inventory`: exact `{sha256,bytes}` objects, opened through verified
  readers. Both original metadata guards remain held throughout association.
- `index_format: "sha256sums"`: strict lowercase SHA-256, one space, text/binary
  marker (` ` or `*`), then a valid relative Tig path. LF and CRLF are accepted.
  GNU escaped-name lines, absolute/traversing paths and unknown formats refuse.
- `inventory_format: "json-lines"`: one object per line with `relative_path`
  and unsigned `bytes`. Additional saved attributes are ignored, never promoted
  to content proof. Duplicate paths/JSON keys and malformed rows refuse.
- `index_prefix`: relative inventory directory containing the indexed files,
  such as `CRM/photos`. Members resolve below this exact directory; no case
  folding, basename search, content-hash deduplication or timestamp selection.
- `target_prefix` and `target_separator` (`slash` or `backslash`): explicit
  literal locator construction. These strings are never opened as filesystem
  paths. A project must separately admit the resulting target scope/policy.
- `verify_payloads`: required boolean. `true` verifies every exact indexed
  object in existing stores; missing/corrupt/over-budget bytes refuse. `false`
  reads metadata only and reports payload verification false. Saved length plus
  an index hash is an assertion, not an observed payload. Aliases may share one
  object, but conflicting lengths for one digest refuse even in metadata-only mode.
- `max_rows` (1–4,096) and `max_output_bytes` (1–16 MiB): explicit output bounds,
  including the terminating newline. Index/inventory bounds are 4/16 MiB,
  with at most 65,536 inventory rows and 64 KiB per inventory line. Profile
  input is at most 64 KiB. Provider budgets can impose stricter limits.

The result's `files` array contains exact `path`, `relative_path`, `asset` and
one-based `checksum_line`, with cache verification and provider-readback flags
kept separate. Use `/files`, `/asset` and `/path` with an admitted registrar
policy. Preserve the result and original index/inventory as asset-side evidence;
do not store the expanding table in Git. An existing target must use its version
update workflow rather than being imported as a duplicate anchor.

Payload checks are sequential named-object observations, not one global atomic
cache snapshot or an independent backup claim. The tool performs no provider
readback, hydration, transfer, production, Git operation, approval, cache write
or eviction. Metadata-only operation opens exactly the two pinned metadata
objects. It does not verify the other payloads merely because their names exist.

The caller owns an existing ordinary private output parent outside configured
byte-store roots and Git internals. New-file creation refuses overwrite; no
directory is created. All association validation finishes before output creation.
Successful output must still be hash-retained before another workflow adopts it.

Producer-applied provenance/privacy review keeps original assertions and saved
observations distinct from actual payload checks and human/provider authority.
Synthetic fixtures cover equal-byte independent targets, missing/corrupt payloads,
metadata-only reads, duplicates/unsafe paths, inconsistent aliases, byte/row/read
limits, private output, no-clobber and portable device/ADS/Git-path refusal. These
tests establish adapter behavior, not project enrollment or full custody coverage.
